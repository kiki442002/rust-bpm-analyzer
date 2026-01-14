use crate::core_bpm::{AudioCapture, AudioMessage, AudioPID, BpmAnalyzer};
use crate::network_sync::LinkManager;
use crate::platform::TARGET_SAMPLE_RATE;
use alsa::Mixer;
use std::sync::mpsc;
use std::time::Duration;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting BPM Analyzer (Headless)...");

    // Paramètres PID à ajuster selon le système
    let mixer = Mixer::new("hw:0", false).map_err(|e: alsa::Error| e.to_string())?;
    let mut pid = AudioPID::new(15.0, 0.0, 0.0, &mixer)?;
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
        Some(Duration::from_millis(250)), // 250ms de données par paquet
    )?;

    println!("Audio capture started. Listening... (Press Ctrl+C to stop)");

    loop {
        match receiver.recv() {
            Ok(AudioMessage::Samples(packet)) => {
                new_samples_accumulator.extend(&packet);
                // PID audio sur chaque paquet de 100ms
                println!(
                    "PID output: {}",
                    pid.update_alsa_from_slice(setpoint, &packet, &mixer)?
                );

                if new_samples_accumulator.len() >= current_hop_size {
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
                    }
                    new_samples_accumulator.clear();
                }
            }
            Ok(AudioMessage::Reset) => {
                println!("Audio stream reset. Clearing buffers...");
                new_samples_accumulator.clear();
            }
            Ok(AudioMessage::SampleRateChanged(rate)) => {
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
            Err(e) => {
                eprintln!("Error receiving audio: {}", e);
                break;
            }
        }
    }

    Ok(())
}
