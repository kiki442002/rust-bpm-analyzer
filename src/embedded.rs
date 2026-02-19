use crate::core_bpm::{AudioCapture, AudioMessage, AudioPID, BpmAnalyzer};
use crate::core_embedded::display::display::BpmDisplay;
use crate::core_embedded::led::led::Led;
use crate::core_embedded::network::network;
use crate::network_sync::{LinkManager, NetworkManager, NetworkMessage};
use crate::platform::TARGET_SAMPLE_RATE;
use alsa::Mixer;
use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::signal;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Initialisation de la LED de statut
    Led::new("/dev/gpiochip4", 2)?.on()?; // Allume une LED de statut

    // Initialisation de l'écran OLED
    let bpm_display: Option<_> = match BpmDisplay::new("/dev/i2c-2") {
        Ok(d) => Some(Arc::new(Mutex::new(d))),
        Err(e) => {
            eprintln!("Erreur init écran OLED: {}", e);
            None
        }
    };

    // Lancement de l'écoute des événements DHCP (si applicable)
    #[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
    {
        tokio::spawn(network::listen_interface_events(bpm_display.clone()));

        // Lancement de l'écoute USB (script custom)
        use crate::core_embedded::usb::usb;
        tokio::spawn(usb::listen_usb_events());
    }

    // // Vérification et application d'une mise à jour si disponible (auto-update)
    // let updater = Updater::new(
    //     "kiki442002",        // Remplace par ton nom d'utilisateur GitHub si besoin
    //     "rust-bpm-analyzer", // Nom du repo GitHub
    //     "rust-bpm-analyzer", // Nom du binaire
    // );
    // let _ = updater.check_and_update();

    // Variable d'arrêt partagée
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_ctrlc = stop_flag.clone();
    // Tâche async qui surveille Ctrl+C
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        println!("Ctrl+C reçu, arrêt demandé.");
        stop_flag_ctrlc.store(true, Ordering::SeqCst);
    });
    println!("Starting BPM Analyzer (Headless)...");

    // Paramètres PID à ajuster selon le système
    let mixer = Mixer::new("hw:0", false).map_err(|e: alsa::Error| e.to_string())?;
    let mut pid = AudioPID::new(15.0, 1.5, 0.0, 8, &mixer)?;
    let setpoint = 0.25; // Niveau cible RMS (à ajuster)

    let (sender, receiver) = mpsc::channel();
    let mut current_hop_size = TARGET_SAMPLE_RATE as usize / 2; // 0.5s par défaut, comme dans gui
    let mut new_samples_accumulator: Vec<f32> = Vec::with_capacity(current_hop_size);
    let mut analyzer = BpmAnalyzer::new(TARGET_SAMPLE_RATE, None)?;
    let mut link_manager = LinkManager::new();
    link_manager.link_state(true); // Active Link

    let _audio_capture = AudioCapture::new(
        sender,
        None,
        TARGET_SAMPLE_RATE,
        None,
        Some(Duration::from_millis(100)), // Réduire à 100ms
    )?;

    // Network Sync
    let binding = NetworkManager::new("embedded_milkv".to_string(), "Milk-V DUOs".to_string());
    if let Err(e) = &binding {
        eprintln!("Network Init Failed: {}", e);
    }
    let network_manager = binding.ok();

    let mut auto_gain_enabled = true; // Enabled by default
    let mut analysis_enabled = true; // Enabled by default

    println!("Audio capture started. Listening... (Press Ctrl+C to stop)");

    for msg in receiver {
        // --- Poll Network Messages ---
        if let Some(net) = &network_manager {
            while let Ok(cmd) = net.try_recv() {
                if !matches!(cmd, NetworkMessage::EnergyLevel(_)) {
                    println!("Network Message Received: {:?}", cmd);
                }
                match cmd {
                    NetworkMessage::SetAutoGain(val) => {
                        println!("Network: SetAutoGain {}", val);
                        auto_gain_enabled = val;
                        let _ = net.announce_presence(true); // Should send current state too
                        let _ = net.send(NetworkMessage::AutoGainState(val));
                    }
                    NetworkMessage::SetAnalysis(val) => {
                        println!("Network: SetAnalysis {}", val);
                        analysis_enabled = val;
                        let _ = net.send(NetworkMessage::AnalysisState(val));
                    }
                    NetworkMessage::Discovery => {
                        let _ = net.announce_presence(true);
                        let _ = net.send(NetworkMessage::AutoGainState(auto_gain_enabled));
                        let _ = net.send(NetworkMessage::AnalysisState(analysis_enabled));
                    }
                    _ => {}
                }
            }
        }

        if stop_flag.load(Ordering::SeqCst) {
            println!("Arrêt demandé, sortie de la boucle.");
            if let Some(net) = &network_manager {
                let _ = net.announce_presence(false);
            }
            break;
        }
        match msg {
            AudioMessage::Samples(packet) => {
                new_samples_accumulator.extend(&packet);

                // --- Calculate RMS / AutoGain ---
                let mut rms = 0.0;
                if auto_gain_enabled {
                    match pid.update_alsa_from_slice(setpoint, &packet, &mixer) {
                        Ok((_, val)) => rms = val,
                        Err(e) => eprintln!("PID update error: {}", e),
                    }
                } else {
                    // Just calculate RMS without adjusting volume
                    rms = (packet.iter().map(|x| x * x).sum::<f32>() / packet.len() as f32).sqrt();
                }

                // --- Send Energy Level ---
                if let Some(net) = &network_manager {
                    // Send energy level to network
                    let _ = net.send(NetworkMessage::EnergyLevel(rms));
                }

                // --- Update Local Display ---
                if let Some(display_mutex) = &bpm_display {
                    // On tente de verrouiller le mutex sans bloquer l'audio
                    if let Ok(mut guard) = display_mutex.try_lock() {
                        let _ = guard.update_audio_bar(rms);
                    }
                }

                // Check analysis enabled
                if !analysis_enabled {
                    new_samples_accumulator.clear();
                } else if new_samples_accumulator.len() >= current_hop_size {
                    if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                        println!(
                            "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2}",
                            result.bpm, result.is_drop, result.confidence, result.coarse_confidence
                        );
                        link_manager.update_tempo(
                            result.bpm as f64,
                            result.is_drop,
                            result.beat_offset,
                        );
                        // Affichage BPM sur l'écran OLED si dispo
                        #[cfg(all(
                            any(target_arch = "aarch64", target_arch = "arm"),
                            target_os = "linux"
                        ))]
                        // L'écran est un Option<Arc<Mutex<BpmDisplay>>>
                        if let Some(display_mutex) = &bpm_display {
                            // On tente de verrouiller le mutex sans bloquer l'audio
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
                        eprintln!("Failed to re-initialize analyzer with rate {}: {}", rate, e)
                    }
                }
            }
        }
    }

    Ok(())
}
