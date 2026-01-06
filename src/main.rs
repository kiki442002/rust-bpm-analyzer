mod core_bpm;
mod network_sync;

// The GUI is the main target for desktop platforms.
#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod gui;
// The embedded module is for ARM-based Linux devices like Raspberry Pi.
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
mod embeded;

use crate::core_bpm::analyzer::BpmAnalyzer;
use crate::core_bpm::audio::{AudioCapture, AudioMessage};
use rubato::{
    Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::sync::mpsc;
use std::time::Duration;

// The analyzer is designed to work at 11025 Hz, matching the original Python implementation.
const TARGET_SAMPLE_RATE: u32 = 11025;
// Process audio in chunks of ~0.4 seconds.
const HOP_SIZE: usize = TARGET_SAMPLE_RATE as usize / 2;

fn main() {
    println!("Starting BPM analyzer...");

    // --- Initialize the BPM Analyzer ---
    let mut analyzer = match BpmAnalyzer::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to initialize BPM analyzer: {}", e);
            return;
        }
    };
    println!("BPM analyzer initialized successfully.");

    // --- Set up Audio Capture ---
    let (sender, receiver) = mpsc::channel();
    // Request the target sample rate, but the audio backend might give us a different one.
    let _audio_capture = match AudioCapture::new(
        sender,
        None,
        TARGET_SAMPLE_RATE,
        None,
        Some(Duration::from_millis(100)),
    ) {
        Ok(ac) => ac,
        Err(e) => {
            eprintln!("Failed to initialize audio capture: {}", e);
            eprintln!("Please ensure you have a working microphone and the necessary permissions.");
            return;
        }
    };
    println!("Audio capture thread started. Waiting for stream to stabilize...");

    // --- Main analysis loop ---
    let mut resampler: Option<SincFixedOut<f32>> = None;
    let mut source_audio_buffer: Vec<f32> = Vec::new();
    let mut resampled_audio_buffer: Vec<f32> = Vec::with_capacity(HOP_SIZE * 2);
    let mut is_initialized = false;

    loop {
        match receiver.recv() {
            Ok(AudioMessage::SampleRateChanged(source_sample_rate)) => {
                println!(
                    "Audio stream active with sample rate: {} Hz",
                    source_sample_rate
                );
                if source_sample_rate != TARGET_SAMPLE_RATE {
                    println!(
                        "Creating resampler for {} -> {} Hz.",
                        source_sample_rate, TARGET_SAMPLE_RATE
                    );
                    let params = SincInterpolationParameters {
                        sinc_len: 128,
                        f_cutoff: 0.95,
                        interpolation: SincInterpolationType::Linear,
                        oversampling_factor: 128,
                        window: WindowFunction::Blackman,
                    };
                    resampler = Some(
                        SincFixedOut::<f32>::new(
                            TARGET_SAMPLE_RATE as f64 / source_sample_rate as f64,
                            2.0,
                            params,
                            1024, // Use a smaller chunk size for lower latency
                            1,    // Mono
                        )
                        .unwrap(),
                    );
                }
                is_initialized = true;
            }
            Ok(AudioMessage::Samples(packet)) => {
                if !is_initialized {
                    continue;
                } // Don't process samples until we have the sample rate

                source_audio_buffer.extend_from_slice(&packet);

                if let Some(resampler) = &mut resampler {
                    while {
                        let needed = resampler.input_frames_next();
                        source_audio_buffer.len() >= needed
                    } {
                        let needed = resampler.input_frames_next();
                        let input_chunk = source_audio_buffer.drain(0..needed).collect::<Vec<_>>();
                        let waves_in = vec![input_chunk];
                        match resampler.process(&waves_in, None) {
                            Ok(resampled) => {
                                for chunk in resampled {
                                    resampled_audio_buffer.extend_from_slice(&chunk);
                                }
                            }
                            Err(e) => {
                                eprintln!("Resampling error: {}", e);
                            }
                        }
                    }
                } else {
                    resampled_audio_buffer.append(&mut source_audio_buffer);
                }

                // Process the buffer when it has enough data
                if resampled_audio_buffer.len() >= HOP_SIZE {
                    if let Some(bpm) = analyzer.process(&resampled_audio_buffer) {
                        println!("\nDetected BPM: {:.2}\n", bpm);
                    } else {
                        println!("BPM detection inconclusive.");
                    }
                    // Keep the tail of the buffer for overlapping windows
                    let drain_len = resampled_audio_buffer.len() - (HOP_SIZE / 2);
                    resampled_audio_buffer.drain(0..drain_len);
                }
            }
            Ok(AudioMessage::Reset) => {
                println!("Audio stream reset. Clearing buffer.");
                source_audio_buffer.clear();
                resampled_audio_buffer.clear();
                is_initialized = false;
            }
            Err(_) => {
                eprintln!("Audio capture channel closed. Exiting.");
                break;
            }
        }
    }
}
