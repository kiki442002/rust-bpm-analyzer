use crate::core_bpm::{AudioCapture, AudioMessage, AudioPID, BpmAnalyzer};
use crate::core_embedded::button::button::{ButtonAction, ButtonListener};
use crate::core_embedded::display::display::BpmDisplay;
use crate::core_embedded::led::led::Led;
use crate::core_embedded::network::network;
use crate::network_sync::LinkManager;
use crate::platform::TARGET_SAMPLE_RATE;
use alsa::Mixer;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::signal;

enum AppEvent {
    Audio(AudioMessage),
    Button(ButtonAction),
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Initialisation de la LED de statut
    if let Err(e) = Led::new("/dev/gpiochip4", 2).and_then(|l| l.on()) {
        eprintln!("Erreur init LED statut: {}", e);
    }

    // Initialisation de l'écran OLED
    let bpm_display: Option<_> = match BpmDisplay::new("/dev/i2c-2") {
        Ok(d) => Some(Arc::new(Mutex::new(d))),
        Err(e) => {
            eprintln!("Erreur init écran OLED: {}", e);
            None
        }
    };

    // Canal principal unique (MPSC Async)
    let (tx_main, mut rx_main) = tokio::sync::mpsc::channel::<AppEvent>(100);

    // Lancement des tâches spécifiques à l'embarqué
    #[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
    {
        /////////////Tache pour événements réseau////////////////
        tokio::spawn(network::listen_interface_events(bpm_display.clone()));
        /////////////////////////////////////////////////////////

        /////////////Tache pour événements USB////////////////
        use crate::core_embedded::usb::usb;
        tokio::spawn(usb::listen_usb_events());
        //////////////////////////////////////////////////////

        /////////////Tache pour événements Bouton////////////////
        let tx_btn = tx_main.clone();
        tokio::spawn(async move {
            let (tx_internal, mut rx_internal) = tokio::sync::mpsc::channel(32);
            let button_listener = ButtonListener::new("/dev/gpiochip4", 3);

            // Lance le listener
            tokio::spawn(async move {
                if let Err(e) = button_listener.run(tx_internal).await {
                    eprintln!("Button listener error: {}", e);
                }
            });

            // Redirige vers la boucle principale
            while let Some(action) = rx_internal.recv().await {
                let _ = tx_btn.send(AppEvent::Button(action)).await;
            }
        });
        ////////////////////////////////////////////////////////
    }

    /////////////Tache pour CTRL+C////////////////
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_ctrlc = stop_flag.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        println!("Ctrl+C reçu, arrêt demandé.");
        stop_flag_ctrlc.store(true, Ordering::SeqCst);
    });
    ////////////////////////////////////////////////

    println!("Starting BPM Analyzer (Headless)...");

    // Paramètres PID
    let mixer = Mixer::new("hw:0", false).map_err(|e: alsa::Error| e.to_string())?;
    let mut pid = AudioPID::new(15.0, 1.5, 0.0, 8, &mixer)?;
    let setpoint = 0.25; // Niveau cible RMS 

    // Ableton Link Manager
    let mut link_manager = LinkManager::new();
    link_manager.link_state(true); // Active Link

    // Analyseur BPM
    let mut analyzer = BpmAnalyzer::new(TARGET_SAMPLE_RATE, None)?;

    // Bridge pour l'Audio (Sync -> Async)
    let (audio_sender, audio_receiver) = mpsc::channel();
    let tx_audio = tx_main.clone();

    // Thread bridge qui convertit les messages Audio (Sync) vers AppEvent (Async)
    std::thread::spawn(move || {
        while let Ok(msg) = audio_receiver.recv() {
            if tx_audio.blocking_send(AppEvent::Audio(msg)).is_err() {
                break;
            }
        }
    });

    // Audio Capture
    let mut current_hop_size = TARGET_SAMPLE_RATE as usize / 2;
    let mut new_samples_accumulator: Vec<f32> = Vec::with_capacity(current_hop_size);
    let _audio_capture = AudioCapture::new(
        audio_sender,
        None,
        TARGET_SAMPLE_RATE,
        None,
        Some(Duration::from_millis(500)),
    )?;

    println!("App initilized, start listening... (Press Ctrl+C to stop)");

    // Boucle Principale Async (Consomme Audio + Boutons)
    while let Some(event) = rx_main.recv().await {
        if stop_flag.load(Ordering::SeqCst) {
            println!("Arrêt demandé, sortie de la boucle.");
            break;
        }

        match event {
            AppEvent::Button(action) => {
                println!(">> Button Action: {:?}", action);
                match action {
                    ButtonAction::SinglePress => {
                        // Action sur simple click (ex: Tap Tempo ?)
                    }
                    ButtonAction::DoublePress => {}
                    ButtonAction::LongPress => {
                        if let Some(display_mutex) = &bpm_display {
                            let mut update_in_progress = Err("Not init".into());
                            // On tente de verrouiller le mutex sans bloquer
                            if let Ok(mut guard) = display_mutex.try_lock() {
                                update_in_progress = guard.update_in_progress();
                            }
                            match update_in_progress {
                                Ok(_) => {
                                    use crate::core_embedded::update::update::Updater;
                                    let updater = Updater::new(
                                        "kiki442002",
                                        "rust-bpm-analyzer",
                                        "rust-bpm-analyzer",
                                    );

                                    let is_running = Arc::new(AtomicBool::new(true));
                                    let _ = tokio::spawn(BpmDisplay::run_update_animation(
                                        display_mutex.clone(),
                                        is_running.clone(),
                                    ));
                                    updater.check_and_update().ok();
                                }
                                Err(e) => eprintln!("Erreur lancement mise à jour: {}", e),
                            }
                        }
                    }
                }
            }
            AppEvent::Audio(msg) => {
                match msg {
                    AudioMessage::Samples(packet) => {
                        new_samples_accumulator.extend(&packet);
                        match pid.update_alsa_from_slice(setpoint, &packet, &mixer) {
                            Ok((_, rms)) => {
                                //println!("PID output gain: {}", gain);
                                if let Some(display_mutex) = &bpm_display {
                                    // On tente de verrouiller le mutex sans bloquer
                                    if let Ok(mut guard) = display_mutex.try_lock() {
                                        let _ = guard.update_audio_bar(rms);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("PID update error: {}", e);
                            }
                        }

                        if new_samples_accumulator.len() >= current_hop_size {
                            if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                                println!(
                                    "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2}",
                                    result.bpm,
                                    result.is_drop,
                                    result.confidence,
                                    result.coarse_confidence
                                );
                                link_manager.update_tempo(
                                    result.bpm as f64,
                                    result.is_drop,
                                    result.beat_offset,
                                );
                                #[cfg(all(
                                    any(target_arch = "aarch64", target_arch = "arm"),
                                    target_os = "linux"
                                ))]
                                if let Some(display_mutex) = &bpm_display {
                                    if let Ok(mut guard) = display_mutex.try_lock() {
                                        let _ = guard.show_bpm(result.bpm);
                                    }
                                }
                            }
                            new_samples_accumulator.clear();
                        }
                    }
                    AudioMessage::Reset => {
                        println!("Audio stream reset. Clearing buffers...");
                        new_samples_accumulator.clear();
                    }
                    AudioMessage::SampleRateChanged(rate) => {
                        println!("Audio sample rate changed to: {} Hz", rate);
                        match BpmAnalyzer::new(rate, None) {
                            Ok(new_analyzer) => {
                                analyzer = new_analyzer;
                                current_hop_size = (rate / 2) as usize;
                                if new_samples_accumulator.capacity() < current_hop_size {
                                    new_samples_accumulator
                                        .reserve(current_hop_size - new_samples_accumulator.len());
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "Failed to re-initialize analyzer with rate {}: {}",
                                    rate, e
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
