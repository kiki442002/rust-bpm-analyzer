pub fn run_headless() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting BPM Analyzer (Headless)...");

    let (sender, receiver) = mpsc::channel();

    let mut current_hop_size = HOP_SIZE;

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
                if new_samples_accumulator.len() >= current_hop_size {
                    // Analyze the new chunk of data
                    if let Ok(Some(result)) = analyzer.process(&new_samples_accumulator) {
                        println!(
                            "BPM: {:.1} | Drop: {} | Conf: {:.2} | CoarseConf: {:.2} | Energy: {:.4} | Avg: {:.4} | Raw: {:.4} | Rise: {:.4}",
                            result.bpm,
                            result.is_drop,
                            result.confidence,
                            result.coarse_confidence,
                            result.energy,
                            result.average_energy,
                            result.raw_energy,
                            result.max_rise,
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
            Ok(AudioMessage::SampleRateChanged(rate)) => {
                println!("Audio sample rate changed to: {} Hz", rate);
                match BpmAnalyzer::new(rate, None) {
                    Ok(new_analyzer) => {
                        analyzer = new_analyzer;
                        current_hop_size = rate as usize;
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
