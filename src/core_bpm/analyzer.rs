//! Logic for analyzing audio buffers to find the BPM.
//! This is a direct port of the Python implementation, with optimizations.

use crate::core_bpm::bpm_pattern::BpmPattern;
use biquad::*;

const TARGET_SAMPLE_RATE: f32 = 11025.0;
const ANALYSIS_BUFFER_SECONDS: f32 = 10.0;
const ANALYSIS_BUFFER_SIZE: usize = (TARGET_SAMPLE_RATE * ANALYSIS_BUFFER_SECONDS) as usize;

/// The main analyzer struct.
pub struct BpmAnalyzer {
    /// Bandpass filter to isolate the relevant frequencies for beat detection.
    filter: AudioFilter,
    /// Coarse BPM patterns (e.g., 100, 101, 102 BPM).
    pattern_coarse: BpmPattern,
    /// Fine BPM patterns for more precise detection (e.g., 100.1, 100.2 BPM).
    pattern_fine: BpmPattern,
    /// Internal buffer to store the filtered audio signal.
    audio_buffer: Vec<f32>,
}

impl BpmAnalyzer {
    /// Creates a new `BpmAnalyzer`.
    ///
    /// It loads the embedded BPM patterns and initializes the audio filter.
    pub fn new() -> Result<Self, String> {
        println!("[BPM ANALYZER] Initializing...");
        let pattern_coarse =
            super::bpm_pattern::BpmPattern::generate(130.0, 230.0, 0.25, 11025 / 2 / 20, 11025);
        let pattern_fine =
            super::bpm_pattern::BpmPattern::generate(130.0, 230.0, 0.05, 11025 / 2 / 20, 11025);

        // Ensure patterns are compatible with the target sample rate.
        if pattern_coarse.frame_rate as f32 != TARGET_SAMPLE_RATE
            || pattern_fine.frame_rate as f32 != TARGET_SAMPLE_RATE
        {
            let err_msg = format!(
                "BPM patterns are for {} Hz, but analyzer requires {} Hz.",
                pattern_coarse.frame_rate, TARGET_SAMPLE_RATE
            );
            println!("[BPM ANALYZER] Error: {}", err_msg);
            return Err(err_msg);
        }

        // The python implementation uses a 6th order butterworth filter.
        // A 4th order biquad filter is a good approximation.
        let filter = AudioFilter::new(
            FilterType::BandPass(60.0, 3000.0),
            TARGET_SAMPLE_RATE,
            FilterOrder::Order4,
        )?;
        println!("[BPM ANALYZER] Filter created.");

        println!("[BPM ANALYZER] Initialization successful.");
        Ok(Self {
            filter,
            pattern_coarse,
            pattern_fine,
            audio_buffer: Vec::with_capacity(ANALYSIS_BUFFER_SIZE),
        })
    }

    /// Analyzes an audio buffer to detect the BPM.
    ///
    /// The input buffer should be mono audio at 11025 Hz.
    ///
    /// # Returns
    ///
    /// An `Option<f32>` with the detected BPM if successful.
    pub fn process(&mut self, audio_buffer: &[f32]) -> Option<f32> {
        // 1. Filter and append new audio samples
        self.audio_buffer
            .extend(audio_buffer.iter().map(|s| self.filter.process(*s)));

        // 2. Keep the buffer size within the `ANALYSIS_BUFFER_SIZE` limit (sliding window)
        if self.audio_buffer.len() > ANALYSIS_BUFFER_SIZE {
            let drain_count = self.audio_buffer.len() - ANALYSIS_BUFFER_SIZE;
            self.audio_buffer.drain(0..drain_count);
        }

        // 3. Check if we have enough data to perform an analysis.
        // The `search_beat_events` function requires at least `step_size` samples.
        let step_size = (self.pattern_coarse.frame_rate / 2) as usize;
        if self.audio_buffer.len() < step_size {
            println!(
                "[BPM ANALYZER] Buffer too small ({}), need at least {}. Waiting for more audio.",
                self.audio_buffer.len(),
                step_size
            );
            return None;
        }

        println!(
            "[BPM ANALYZER] Processing buffer of size: {}",
            self.audio_buffer.len()
        );

        // 4. Perform the two-pass (coarse, then fine) BPM search
        self.search_bpm(&self.audio_buffer)
    }

    /// The core search logic, ported from the Python implementation.
    fn search_bpm(&self, signal: &[f32]) -> Option<f32> {
        // --- Pass 1: Coarse BPM Search ---
        println!("[BPM ANALYZER] Starting coarse BPM search...");

        // Detect beat events (energy peaks) in the signal
        let beat_events = self.search_beat_events(signal);
        println!("[BPM ANALYZER] Found {} beat events.", beat_events.len());
        if beat_events.is_empty() {
            println!("[BPM ANALYZER] No beat events found, aborting.");
            return None;
        }

        // Correlate beat events with the coarse pattern
        let bpm_container_coarse = self.bpm_container(&beat_events, &self.pattern_coarse.data);
        let bpm_container_wrapped_coarse =
            self.wrap_bpm_container(&bpm_container_coarse, self.pattern_coarse.data.len());
        let bpm_container_final_coarse = self.finalise_bpm_container(&bpm_container_wrapped_coarse);

        // Find the most likely BPM index from the coarse search
        let bpm_wrapped_coarse = self.get_bpm_wrapped(&bpm_container_final_coarse);
        println!(
            "[BPM ANALYZER] Coarse BPM wrapped result: {:?}",
            bpm_wrapped_coarse
        );

        // Check if the result is valid
        let is_valid = self.check_bpm_wrapped(&bpm_wrapped_coarse, &bpm_container_final_coarse);
        println!("[BPM ANALYZER] Coarse BPM result is valid: {}", is_valid);
        if !is_valid {
            return None;
        }

        // --- Pass 2: Fine BPM Search ---
        println!("[BPM ANALYZER] Starting fine BPM search...");

        // Determine the window in the fine pattern to search, based on the coarse result
        let (fine_start, fine_end) = self.get_bpm_pattern_fine_window(&bpm_wrapped_coarse);
        println!(
            "[BPM ANALYZER] Fine BPM search window: {} to {}",
            fine_start, fine_end
        );
        if fine_start >= fine_end {
            println!("[BPM ANALYZER] Invalid fine window, aborting.");
            return None;
        }
        let fine_pattern_window = &self.pattern_fine.data[fine_start..fine_end];

        // Correlate beat events with the fine pattern window
        let bpm_container_fine = self.bpm_container(&beat_events, fine_pattern_window);
        let bpm_container_wrapped_fine =
            self.wrap_bpm_container(&bpm_container_fine, fine_pattern_window.len());
        let bpm_container_final_fine = self.finalise_bpm_container(&bpm_container_wrapped_fine);

        // Find the most likely BPM index from the fine search
        let bpm_wrapped_fine = self.get_bpm_wrapped(&bpm_container_final_fine);
        println!(
            "[BPM ANALYZER] Fine BPM wrapped result: {:?}",
            bpm_wrapped_fine
        );

        // In Python, the second pass check was more lenient. Here we just check for emptiness.
        if bpm_wrapped_fine.is_empty() {
            println!("[BPM ANALYZER] No fine BPM result found, aborting.");
            return None;
        }

        // --- Finalization ---

        // Convert the coarse and fine indices to a final BPM value
        let bpm_float =
            self.bpm_indices_to_float(bpm_wrapped_coarse[0], fine_start + bpm_wrapped_fine[0]);
        println!("[BPM ANALYZER] Final BPM: {}", bpm_float);

        Some(bpm_float)
    }

    /// Detects prominent beat events in the audio signal.
    /// This is equivalent to finding the `argmax` in sliding windows.
    fn search_beat_events(&self, signal: &[f32]) -> Vec<usize> {
        let step_size = (self.pattern_coarse.frame_rate / 2) as usize;
        if signal.len() < step_size {
            return Vec::new();
        }

        signal
            .windows(step_size)
            .step_by(step_size)
            .enumerate()
            .map(|(i, window)| {
                let max_pos = window
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);
                i * step_size + max_pos
            })
            .collect()
    }

    /// Correlates beat events with BPM patterns.
    /// This is an optimized version of the Python `bpm_container` function.
    fn bpm_container(
        &self,
        beat_events: &[usize],
        bpm_pattern: &[Vec<Vec<u32>>],
    ) -> Vec<Vec<usize>> {
        let steps = bpm_pattern.len();
        let mut bpm_container: Vec<Vec<usize>> = vec![Vec::new(); beat_events.len() * steps];

        for (i, &beat_event) in beat_events.iter().enumerate() {
            let beat_event_s = beat_event as isize;
            let lower_bound = (beat_event_s - 20).max(0) as u32;
            let upper_bound = (beat_event_s + 20) as u32;

            for q in 0..steps {
                // q is bpm_index
                for (x, pattern) in bpm_pattern[q].iter().enumerate() {
                    // x is lengh_index
                    // Use binary search (partition_point) to find matches efficiently,
                    // since the inner `pattern` vector is sorted.
                    let start_idx = pattern.partition_point(|&p| p < lower_bound);
                    let end_idx = pattern.partition_point(|&p| p <= upper_bound);
                    let count = end_idx - start_idx;

                    if count > 0 {
                        // Push `x` (the lengh_index) `count` times.
                        let sub_container = &mut bpm_container[i * steps + q];
                        sub_container.resize(sub_container.len() + count, x);
                    }
                }
            }
        }
        bpm_container
    }

    /// Reorganizes the container to group results by BPM hypothesis.
    fn wrap_bpm_container(&self, bpm_container: &[Vec<usize>], steps: usize) -> Vec<Vec<usize>> {
        let mut bpm_container_wrapped: Vec<Vec<usize>> = vec![Vec::new(); steps];
        for i in 0..steps {
            for j in (i..bpm_container.len()).step_by(steps) {
                bpm_container_wrapped[i].extend_from_slice(&bpm_container[j]);
            }
        }
        bpm_container_wrapped
    }

    /// Counts the most frequent "lengh_index" for each BPM hypothesis.
    fn finalise_bpm_container(&self, bpm_container_wrapped: &[Vec<usize>]) -> Vec<usize> {
        bpm_container_wrapped
            .iter()
            .map(|w| {
                if w.is_empty() {
                    return 0;
                }
                let mut counts = std::collections::HashMap::new();
                for &val in w {
                    *counts.entry(val).or_insert(0) += 1;
                }
                // Return the count of the most frequent element.
                counts.into_values().max().unwrap_or(0)
            })
            .collect()
    }

    /// Finds the index/indices corresponding to the highest score.
    fn get_bpm_wrapped(&self, bpm_container_final: &[usize]) -> Vec<usize> {
        let max_val = match bpm_container_final.iter().max() {
            Some(&v) => v,
            None => return Vec::new(),
        };

        if max_val == 0 {
            return Vec::new();
        }

        bpm_container_final
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| if v == max_val { Some(i) } else { None })
            .collect()
    }

    /// Checks if the coarse BPM result is valid.
    /// Ported directly from the Python implementation's logic.
    fn check_bpm_wrapped(&self, bpm_wrapped: &[usize], bpm_container_final: &[usize]) -> bool {
        if bpm_wrapped.is_empty() {
            return false;
        }
        // In python: `count > 1 or bpm_container_final[int(bpm_wrapped[0][0])] < 6` means failure.
        // So success is: `count == 1 and score >= 6`.
        let count = bpm_wrapped.len();
        let score = bpm_container_final[bpm_wrapped[0]];

        count == 1 && score >= 6
    }

    /// Calculates the search window within the fine pattern based on the coarse result.
    /// Ported directly from the Python implementation's logic.
    fn get_bpm_pattern_fine_window(&self, bpm_wrapped_coarse: &[usize]) -> (usize, usize) {
        if bpm_wrapped_coarse.is_empty() {
            return (0, 0);
        }
        // Formula from python: `int(((bpm_wrapped[0][0] / 4) / 0.05) - 20)`
        let coarse_index = bpm_wrapped_coarse[0];
        let coarse_bpm_value = self.pattern_coarse.start_bpm - 2.0
            + ((coarse_index) as f32 * self.pattern_coarse.step);
        println!(
            "[BPM ANALYZER] Coarse BPM value for fine window calculation: {}",
            coarse_bpm_value
        );

        let fine_equivalent_index = ((coarse_bpm_value - (self.pattern_fine.start_bpm - 10.0))
            / self.pattern_fine.step)
            .round() as isize;

        let start = (fine_equivalent_index - 20).max(0) as usize;
        let end = (start + 40).min(self.pattern_fine.data.len());

        (start, end)
    }

    /// Converts the final coarse and fine indices to a BPM value.
    fn bpm_indices_to_float(&self, _coarse_index: usize, fine_index: usize) -> f32 {
        // Ajout du /4 pour coller à la logique Python
        let bpm_float =
            self.pattern_fine.start_bpm - 10.0 + ((fine_index) as f32 * self.pattern_fine.step);
        // Arrondi à 2 décimales
        (bpm_float * 100.0).round() / 100.0
    }
}

/// A simple wrapper for a biquad filter chain.
pub struct AudioFilter {
    chain: Vec<DirectForm2Transposed<f32>>,
}

impl AudioFilter {
    pub fn new(
        filter_type: FilterType,
        sample_rate: f32,
        order: FilterOrder,
    ) -> Result<Self, String> {
        let mut chain = Vec::new();
        let sections_count = match order {
            FilterOrder::Order2 => 1,
            FilterOrder::Order4 => 2, // 2 sections for a 4th order filter
        };

        for _ in 0..sections_count {
            let FilterType::BandPass(low, high) = filter_type;
            let fs = Hertz::<f32>::from_hz(sample_rate).map_err(|e| format!("{:?}", e))?;
            let f_low = Hertz::<f32>::from_hz(low).map_err(|e| format!("{:?}", e))?;
            let f_high = Hertz::<f32>::from_hz(high).map_err(|e| format!("{:?}", e))?;

            // Create a high-pass and a low-pass to form a band-pass
            let hp_coeffs =
                Coefficients::<f32>::from_params(Type::HighPass, fs, f_low, Q_BUTTERWORTH_F32)
                    .map_err(|e| format!("{:?}", e))?;
            chain.push(DirectForm2Transposed::<f32>::new(hp_coeffs));

            let lp_coeffs =
                Coefficients::<f32>::from_params(Type::LowPass, fs, f_high, Q_BUTTERWORTH_F32)
                    .map_err(|e| format!("{:?}", e))?;
            chain.push(DirectForm2Transposed::<f32>::new(lp_coeffs));
        }
        Ok(Self { chain })
    }

    fn process(&mut self, sample: f32) -> f32 {
        self.chain
            .iter_mut()
            .fold(sample, |acc, filter| filter.run(acc))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum FilterType {
    BandPass(f32, f32), // Low Cutoff, High Cutoff
}

#[derive(Clone, Copy, Debug)]
pub enum FilterOrder {
    Order2,
    Order4,
}
