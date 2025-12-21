mod core_bpm;
mod network_sync;

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod gui;

use core_bpm::AudioCapture;
use core_bpm::BpmAnalyzer;
use core_bpm::audio::AudioMessage;
use network_sync::LinkManager;
use std::sync::mpsc;
use std::time::Duration;

// Configuration based on target architecture
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
const SAMPLE_RATE: u32 = 11025; // embedded Linux 

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
const SAMPLE_RATE: u32 = 44100; // Development (Mac/PC/Linux)

const HOP_SIZE: usize = SAMPLE_RATE as usize; // Update every 1 second

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
    {
        println!("Starting GUI Mode...");
        gui::run()
    }

    #[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
    {
        println!("Starting Headless Mode...");
        run_headless()
    }
}

fn run_headless() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting BPM Analyzer (Headless)...");

    let (sender, receiver) = mpsc::channel();

    // Temporary buffer to collect new samples until we reach HOP_SIZE
    let mut new_samples_accumulator: Vec<f32> = Vec::with_capacity(HOP_SIZE);

    // Initialize BPM Analyzer
    let mut analyzer = BpmAnalyzer::new(SAMPLE_RATE, None)?;

    // Initialize Ableton Link
    let mut link_manager = LinkManager::new();
    link_manager.link_state(true); // Enable Link

    // Use default device (None) and default restart policy (None)
    // Request a buffer size of 500ms to reduce latency
    let _audio_capture = AudioCapture::new(
        sender,
        None,
        SAMPLE_RATE,
        None,
        Some(Duration::from_millis(500)),
    )?;

    println!("Audio capture started. Listening... (Press Ctrl+C to stop)");

    // Simple loop to consume data
    loop {
        match receiver.recv() {
            Ok(AudioMessage::Samples(packet)) => {
                // Accumulate new samples
                new_samples_accumulator.extend(packet);

                // When we have enough new samples (1 second worth)
                if new_samples_accumulator.len() >= HOP_SIZE {
                    // Analyze the new chunk of data
                    if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                        println!(
                            "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2} | Energy: {:.4} | Avg: {:.4}",
                            result.bpm,
                            result.is_drop,
                            result.confidence,
                            result.coarse_confidence,
                            result.energy,
                            result.average_energy,
                        );

                        // Sync Ableton Link
                        link_manager.update_tempo(
                            result.bpm as f64,
                            result.is_drop,
                            result.beat_offset,
                        );
                    }

                    // Clear accumulator for next batch
                    new_samples_accumulator.clear();
                }
            }
            Ok(AudioMessage::Reset) => {
                println!("Audio stream reset. Clearing buffers...");
                new_samples_accumulator.clear();
            }
            Err(e) => {
                eprintln!("Error receiving audio: {}", e);
                break;
            }
        }
    }

    Ok(())
}
