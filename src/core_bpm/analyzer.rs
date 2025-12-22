use biquad::*;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct BpmHistoryEntry {
    bpm: f32,
    energy: f32,
    timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct AnalysisResult {
    pub bpm: f32,
    pub is_drop: bool,
    pub confidence: f32,
    pub coarse_confidence: f32,
    pub energy: f32,
    pub average_energy: f32,
    pub beat_offset: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub struct NormalizationResult {
    pub energy_sum: f32,
    pub energy_mean: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BpmAnalyzerConfig {
    pub window_duration: Duration,
    pub min_bpm: f32,
    pub max_bpm: f32,
    pub thresholds: ConfidenceThreshold,
}

impl Default for BpmAnalyzerConfig {
    fn default() -> Self {
        Self {
            window_duration: Duration::from_secs(4),
            min_bpm: 60.0,
            max_bpm: 310.0,
            thresholds: ConfidenceThreshold {
                fine_confidence: 0.3,
                coarse_confidence: 0.4,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum FilterType {
    LowPass(f32),       // Cutoff
    HighPass(f32),      // Cutoff
    BandPass(f32, f32), // Low Cutoff, High Cutoff
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum FilterOrder {
    Order2,
    Order4,
}
#[derive(Clone, Copy, Debug)]
pub struct ConfidenceThreshold {
    pub fine_confidence: f32,
    pub coarse_confidence: f32,
}

#[derive(Clone, Debug)]
pub struct SamplingConfig {
    pub buffer: VecDeque<f32>,
    pub rate: f32,
    pub step: usize,
    pub min_lag: usize,
    pub max_lag: usize,
}
impl SamplingConfig {
    pub fn new(rate: f32, duration: Duration, step: usize, min_bpm: f32, max_bpm: f32) -> Self {
        let capacity = (rate * duration.as_secs_f32()) as usize;
        let min_lag = (rate * 60.0 / max_bpm) as usize;
        let max_lag = (rate * 60.0 / min_bpm) as usize;

        Self {
            buffer: VecDeque::with_capacity(capacity),
            rate,
            step,
            min_lag,
            max_lag,
        }
    }

    pub fn update_buffer<F>(&mut self, samples: &[f32], output: &mut Vec<f32>, mut transform: F)
    where
        F: FnMut(&[f32]) -> f32,
    {
        output.clear();

        for chunk in samples.chunks(self.step) {
            let val = transform(chunk);
            output.push(val);
        }

        for &sample in output.iter() {
            if self.buffer.len() >= self.buffer.capacity() {
                self.buffer.pop_front();
            }
            self.buffer.push_back(sample);
        }
    }
}

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

        // L'ordre doit être un multiple de 2 car chaque section biquad est d'ordre 2
        // Si order = 2 -> 1 section
        // Si order = 4 -> 2 sections
        let sections_count = match order {
            FilterOrder::Order2 => 1,
            FilterOrder::Order4 => 2,
        };

        for _ in 0..sections_count {
            match filter_type {
                FilterType::LowPass(cutoff) => {
                    let fs = Hertz::<f32>::from_hz(sample_rate)
                        .map_err(|_| "Invalid sample rate".to_string())?;
                    let f0 = Hertz::<f32>::from_hz(cutoff)
                        .map_err(|_| "Invalid cutoff frequency".to_string())?;

                    let coeffs =
                        Coefficients::<f32>::from_params(Type::LowPass, fs, f0, Q_BUTTERWORTH_F32)
                            .map_err(|e| format!("LP Error: {:?}", e))?;
                    chain.push(DirectForm2Transposed::<f32>::new(coeffs));
                }
                FilterType::HighPass(cutoff) => {
                    let fs = Hertz::<f32>::from_hz(sample_rate)
                        .map_err(|_| "Invalid sample rate".to_string())?;
                    let f0 = Hertz::<f32>::from_hz(cutoff)
                        .map_err(|_| "Invalid cutoff frequency".to_string())?;

                    let coeffs =
                        Coefficients::<f32>::from_params(Type::HighPass, fs, f0, Q_BUTTERWORTH_F32)
                            .map_err(|e| format!("HP Error: {:?}", e))?;
                    chain.push(DirectForm2Transposed::<f32>::new(coeffs));
                }
                FilterType::BandPass(low, high) => {
                    let fs = Hertz::<f32>::from_hz(sample_rate)
                        .map_err(|_| "Invalid sample rate".to_string())?;
                    let f_low = Hertz::<f32>::from_hz(low)
                        .map_err(|_| "Invalid low cutoff frequency".to_string())?;
                    let f_high = Hertz::<f32>::from_hz(high)
                        .map_err(|_| "Invalid high cutoff frequency".to_string())?;

                    let hp_coeffs = Coefficients::<f32>::from_params(
                        Type::HighPass,
                        fs,
                        f_low,
                        Q_BUTTERWORTH_F32,
                    )
                    .map_err(|e| format!("BP-HP Error: {:?}", e))?;

                    let lp_coeffs = Coefficients::<f32>::from_params(
                        Type::LowPass,
                        fs,
                        f_high,
                        Q_BUTTERWORTH_F32,
                    )
                    .map_err(|e| format!("BP-LP Error: {:?}", e))?;

                    chain.push(DirectForm2Transposed::<f32>::new(hp_coeffs));
                    chain.push(DirectForm2Transposed::<f32>::new(lp_coeffs));
                }
            }
        }

        Ok(Self { chain })
    }
    fn process(&mut self, sample: f32) -> f32 {
        let mut out = sample;
        for filter in &mut self.chain {
            out = filter.run(out);
        }
        out
    }
}

pub struct BpmAnalyzer {
    // Configuration
    pub config: BpmAnalyzerConfig,

    // Historique structuré (BPM, Energie, Temps)
    history: VecDeque<BpmHistoryEntry>,

    // Sampling Configs (Buffers + Rates)
    fine_config: SamplingConfig,
    coarse_config: SamplingConfig,

    // Filtre Principal
    input_filter: AudioFilter,

    // Reference BPM (Lock sur Drop)
    reference_bpm: f32,

    // Scratch buffers for memory optimization
    scratch_fine_vec: Vec<f32>,
    scratch_fine_centered: Vec<f32>,
    scratch_coarse_vec: Vec<f32>,
    scratch_coarse_centered: Vec<f32>,
    scratch_processing: Vec<f32>,
    scratch_bpm_sort: Vec<f32>,
}

impl BpmAnalyzer {
    pub fn new(
        sample_rate: u32,
        config: Option<BpmAnalyzerConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config = config.unwrap_or_default();

        // Stratégie Coarse-Fine
        // Fine Rate : ~11000 Hz (Compromis Précision/CPU)
        // Coarse Rate : ~500 Hz (Recherche rapide)

        // Pour 44100Hz : Step 4 => 11025 Hz. Pour 110025Hz : Step 1 => 110025Hz Hz.
        let fine_step = if sample_rate >= 44100 { 4 } else { 1 };

        // Pour garder ~500Hz en Coarse :
        // 11025 / 22 ~= 501 Hz.
        // 8000 / 16 = 500 Hz.
        let coarse_step = 22;

        let fine_rate = sample_rate as f32 / fine_step as f32;
        let coarse_rate = fine_rate / coarse_step as f32;
        let window_duration = config.window_duration;

        let fine_config = SamplingConfig::new(
            fine_rate,
            window_duration,
            fine_step,
            config.min_bpm,
            config.max_bpm,
        );
        let coarse_config = SamplingConfig::new(
            coarse_rate,
            window_duration,
            coarse_step,
            config.min_bpm,
            config.max_bpm,
        );

        // Configuration du filtre principal : BandPass 50Hz - 250Hz
        let input_filter = AudioFilter::new(
            FilterType::BandPass(50.0, 250.0),
            sample_rate as f32,
            FilterOrder::Order4,
        )?;

        println!("BPM Analyzer Configured:");
        println!("  Sample Rate: {} Hz", sample_rate);
        println!("  Fine Rate: {:.2} Hz (Step {})", fine_rate, fine_step);
        println!(
            "  Coarse Rate: {:.2} Hz (Step {})",
            coarse_rate, coarse_step
        );

        Ok(Self {
            config,
            history: VecDeque::with_capacity(5),
            fine_config,
            coarse_config,
            input_filter,
            reference_bpm: 0.0,
            scratch_fine_vec: Vec::with_capacity(4096),
            scratch_fine_centered: Vec::with_capacity(4096),
            scratch_coarse_vec: Vec::with_capacity(1024),
            scratch_coarse_centered: Vec::with_capacity(1024),
            scratch_processing: Vec::with_capacity(1024),
            scratch_bpm_sort: Vec::with_capacity(5),
        })
    }

    fn normalize_window(
        buffer: &VecDeque<f32>,
        out_vec: &mut Vec<f32>,
        out_centered: &mut Vec<f32>,
    ) -> NormalizationResult {
        out_vec.clear();
        out_vec.extend(buffer.iter());

        // 1. Find Max
        let raw_max = out_vec.iter().cloned().fold(0.0 / 0.0, f32::max);

        // 2. Normalize to 0..1
        if raw_max > 0.0 {
            for x in out_vec.iter_mut() {
                *x /= raw_max;
            }
        }

        // 3. Center (Remove DC offset)
        let mean: f32 = out_vec.iter().sum::<f32>() / out_vec.len() as f32;

        out_centered.clear();
        out_centered.extend(out_vec.iter().map(|x| x - mean));

        // 4. Calculate Energy
        let energy_sum: f32 = out_centered.iter().map(|x| x * x).sum();
        let energy_mean = if !out_centered.is_empty() {
            energy_sum / out_centered.len() as f32
        } else {
            0.0
        };

        NormalizationResult {
            energy_sum,
            energy_mean,
        }
    }

    fn search_correlation(
        &self,
        centered_signal: &[f32],
        energy: f32,
        min_lag: usize,
        max_lag: usize,
        min_confidence: f32,
    ) -> Result<(usize, f32, f32), &'static str> {
        let safe_max_lag = centered_signal.len().saturating_sub(1);
        let start_lag = min_lag.max(1);
        let end_lag = max_lag.min(safe_max_lag);

        let mut best_lag = 0;
        let mut max_corr = 0.0;

        for lag in start_lag..=end_lag {
            let mut corr = 0.0;
            for i in 0..(centered_signal.len() - lag) {
                corr += centered_signal[i] * centered_signal[i + lag];
            }
            if corr > max_corr {
                max_corr = corr;
                best_lag = lag;
            }
        }

        if best_lag == 0 {
            return Err("No correlation found");
        }

        let confidence = if energy > 0.0 { max_corr / energy } else { 0.0 };

        if confidence < min_confidence {
            return Err("Confidence too low");
        }

        Ok((best_lag, confidence, max_corr))
    }

    fn check_harmonics(
        &self,
        initial_lag: usize,
        initial_corr: f32,
        centered_signal: &[f32],
        min_lag: usize,
    ) -> usize {
        let mut best_lag = initial_lag;

        // Helper closure for local search
        let find_best_in_range = |center_lag: usize| -> (usize, f32) {
            let start = center_lag.saturating_sub(1);
            let end = center_lag + 1;
            let mut max_c = 0.0;
            let mut best_l = 0;

            for lag in start..=end {
                if lag >= centered_signal.len() {
                    continue;
                }
                let mut corr = 0.0;
                for i in 0..(centered_signal.len() - lag) {
                    corr += centered_signal[i] * centered_signal[i + lag];
                }
                if corr > max_c {
                    max_c = corr;
                    best_l = lag;
                }
            }
            (best_l, max_c)
        };

        // 1. Check 2x BPM (Half Lag)
        let half_lag = initial_lag / 2;
        if half_lag >= min_lag {
            let (best_half_lag, max_half_corr) = find_best_in_range(half_lag);
            if max_half_corr > (initial_corr * 0.5) {
                best_lag = best_half_lag;
            }
        }

        // 2. Check 3x BPM (Third Lag)
        let third_lag = initial_lag / 3;
        if third_lag >= min_lag {
            let (best_third_lag, max_third_corr) = find_best_in_range(third_lag);
            if max_third_corr > (initial_corr * 0.6) {
                best_lag = best_third_lag;
            }
        }

        best_lag
    }

    fn parabolic_interpolation(
        &self,
        best_lag: usize,
        max_corr: f32,
        centered_signal: &[f32],
        start_lag: usize,
        end_lag: usize,
    ) -> f32 {
        let mut refined_lag = best_lag as f32;

        if best_lag > start_lag && best_lag < end_lag {
            let calc_corr = |l: usize| -> f32 {
                let mut c = 0.0;
                for i in 0..(centered_signal.len() - l) {
                    c += centered_signal[i] * centered_signal[i + l];
                }
                c
            };

            let y_prev = calc_corr(best_lag - 1);
            let y_curr = max_corr;
            let y_next = calc_corr(best_lag + 1);

            let denominator = 2.0 * (y_prev - 2.0 * y_curr + y_next);
            if denominator.abs() > 0.0001 {
                let offset = (y_prev - y_next) / denominator;
                refined_lag = best_lag as f32 + offset;
            }
        }
        refined_lag
    }

    fn check_energy_threshold(&self, current_energy: f32) -> Option<f32> {
        // Calcul de l'énergie moyenne actuelle de l'historique
        let avg_history_energy = if self.history.is_empty() {
            0.0
        } else {
            self.history.iter().map(|e| e.energy).sum::<f32>() / self.history.len() as f32
        };

        // Adaptive Energy Threshold (Gate)
        if !self.history.is_empty()
            && current_energy < (avg_history_energy * 0.9)
            && current_energy < 0.03
        {
            return None;
        }

        Some(avg_history_energy)
    }

    fn check_drop(&self, samples: &[f32], threshold: Option<f32>) -> bool {
        let split_index = (samples.len() * 3) / 4; // 75% du buffer

        let threshold = threshold.unwrap_or(1.3);

        // 1. Énergie de l'historique (0..75%)
        let mut history_sum_sq = 0.0;
        for i in 0..split_index {
            let val = samples[i];
            history_sum_sq += val * val;
        }
        let history_count = split_index.max(1);
        let history_energy = history_sum_sq / history_count as f32;

        // 2. Énergie récente (75%..100%)
        let mut recent_sum_sq = 0.0;
        for i in split_index..samples.len() {
            let val = samples[i];
            recent_sum_sq += val * val;
        }
        let recent_count = (samples.len() - split_index).max(1);
        let current_energy = recent_sum_sq / recent_count as f32;

        // 3. Détection
        (current_energy > history_energy * threshold) && (current_energy > 0.01)
    }

    fn update_and_check_reference(&mut self, bpm: f32, is_drop: bool) -> bool {
        if is_drop {
            // Si c'est un Drop, on met à jour la référence
            self.reference_bpm = bpm;
            true
        } else if self.reference_bpm > 0.0 {
            // Si ce n'est pas un Drop mais qu'on a une référence, on vérifie la cohérence
            let test_ref = self.reference_bpm * 0.1;
            let is_close = (bpm - self.reference_bpm).abs() <= test_ref;
            // Vérification des harmoniques (x2, /2, x3)
            let is_double = (bpm - self.reference_bpm * 2.0).abs() <= test_ref / 2.0;
            let is_half = (bpm - self.reference_bpm / 2.0).abs() <= test_ref * 2.0;
            let is_triple = (bpm - self.reference_bpm * 3.0).abs() <= test_ref / 3.0;

            if !is_close && !is_double && !is_half && !is_triple {
                // BPM incohérent avec la référence -> On ignore cette détection
                false
            } else {
                true
            }
        } else {
            // Pas de référence encore, on ignore
            false
        }
    }

    pub fn process(
        &mut self,
        new_samples: &[f32],
    ) -> Result<Option<AnalysisResult>, Box<dyn std::error::Error>> {
        // 1. Filtrage et Downsampling (Input -> Fine)
        self.fine_config
            .update_buffer(new_samples, &mut self.scratch_processing, |chunk| {
                let mut sum = 0.0;
                for &x in chunk {
                    // Application du filtre
                    let y = self.input_filter.process(x);
                    sum += y.abs(); // Rectification
                }
                sum / chunk.len() as f32
            });

        // 2. Downsampling (Fine -> Coarse)
        // On utilise scratch_coarse_vec comme buffer temporaire pour la sortie de cette étape
        // car il sera écrasé lors de la normalisation coarse juste après.
        self.coarse_config.update_buffer(
            &self.scratch_processing,
            &mut self.scratch_coarse_vec,
            |chunk| {
                let sum: f32 = chunk.iter().sum();
                sum / chunk.len() as f32
            },
        );

        // On attend que le buffer soit plein
        if self.coarse_config.buffer.len() < self.coarse_config.buffer.capacity() {
            return Ok(None);
        }

        // ============================================================
        // ÉTAPE 1 : RECHERCHE GROSSIÈRE (COARSE)
        // ============================================================

        let norm_res_coarse = Self::normalize_window(
            &self.coarse_config.buffer,
            &mut self.scratch_coarse_vec,
            &mut self.scratch_coarse_centered,
        );

        if norm_res_coarse.energy_mean <= 0.001 {
            return Ok(None);
        }

        let (best_lag_c, coarse_conf, max_corr_c) = match self.search_correlation(
            &self.scratch_coarse_centered,
            norm_res_coarse.energy_sum,
            self.coarse_config.min_lag,
            self.coarse_config.max_lag,
            self.config.thresholds.coarse_confidence,
        ) {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        // Correction d'octave (Harmonic Check)
        let best_lag_c = self.check_harmonics(
            best_lag_c,
            max_corr_c,
            &self.scratch_coarse_centered,
            self.coarse_config.min_lag,
        );
        // ============================================================
        // ÉTAPE 2 : RAFFINEMENT (FINE)
        // ============================================================

        // Conversion du Lag Coarse vers Fine
        // Ratio = fine_rate / coarse_rate = coarse_step
        let center_lag_f = best_lag_c * self.coarse_config.step;

        // Fenêtre de recherche Fine
        let search_radius = 50;
        let min_lag_f = center_lag_f.saturating_sub(search_radius);
        let max_lag_f = center_lag_f + search_radius;

        let norm_res_fine = Self::normalize_window(
            &self.fine_config.buffer,
            &mut self.scratch_fine_vec,
            &mut self.scratch_fine_centered,
        );

        // On s'assure de rester dans les bornes du buffer
        let safe_max_lag = self.scratch_fine_centered.len().saturating_sub(1);
        let start_lag = min_lag_f.max(1);
        let end_lag = max_lag_f.min(safe_max_lag);

        let (best_lag_f, confidence, max_corr_f) = match self.search_correlation(
            &self.scratch_fine_centered,
            norm_res_fine.energy_sum,
            min_lag_f,
            max_lag_f,
            self.config.thresholds.fine_confidence,
        ) {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        // ============================================================
        // ÉTAPE 3 : INTERPOLATION PARABOLIQUE
        // ============================================================

        let refined_lag = self.parabolic_interpolation(
            best_lag_f,
            max_corr_f,
            &self.scratch_fine_centered,
            start_lag,
            end_lag,
        );

        // Calcul final du BPM arrondi à 0.1 près
        let bpm = (self.fine_config.rate * 60.0 / refined_lag * 10.0).round() / 10.0;

        // ============================================================
        // DÉTECTION DE DROP (AMÉLIORÉE - Comparaison Intra-Fenêtre)
        // ============================================================
        // On calcule le Drop AVANT de valider le BPM pour l'historique
        // On augmente le seuil (1.5 au lieu de 1.3) et on demande une confiance minimale

        let is_drop = confidence > 0.5 && self.check_drop(&self.scratch_fine_vec, Some(1.5));

        // ============================================================
        // GESTION DE L'HISTORIQUE ET LISSAGE
        // ============================================================

        let now = Instant::now();
        // 1. Reset si silence prolongé (> 10s)
        if let Some(last_entry) = self.history.back() {
            if now.duration_since(last_entry.timestamp).as_secs_f32() > 10.0 {
                self.history.clear();
                self.reference_bpm = 0.0;
            }
        }

        // 2. Vérification du seuil d'énergie adaptatif
        let avg_history_energy = match self.check_energy_threshold(norm_res_fine.energy_mean) {
            Some(e) => e,
            None => return Ok(None),
        };

        // 4. Filtrage par Référence (Lock sur Drop)
        if !self.update_and_check_reference(bpm, is_drop) {
            return Ok(None);
        }

        // 5. Mise à jour de l'historique
        if self.history.len() >= 5 {
            self.history.pop_front();
        }
        self.history.push_back(BpmHistoryEntry {
            bpm: bpm,
            energy: norm_res_fine.energy_mean,
            timestamp: now,
        });

        // 6. Calcul des valeurs lissées
        // Median BPM
        self.scratch_bpm_sort.clear();
        self.scratch_bpm_sort
            .extend(self.history.iter().map(|e| e.bpm));
        self.scratch_bpm_sort
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let smoothed_bpm = if !self.scratch_bpm_sort.is_empty() {
            self.scratch_bpm_sort[self.scratch_bpm_sort.len() / 2]
        } else {
            bpm
        };

        // Calcul de l'offset précis du beat (Latence)
        // Si c'est un Drop, on cherche le pic dans la section récente (derniers 25%)
        // pour éviter de se caler sur un pic ancien.
        let search_start = if is_drop {
            (self.scratch_fine_vec.len() * 3) / 4
        } else {
            0
        };

        let mut max_energy = 0.0;
        let mut max_energy_index = search_start;
        for (i, &val) in self.scratch_fine_vec.iter().enumerate().skip(search_start) {
            if val > max_energy {
                max_energy = val;
                max_energy_index = i;
            }
        }

        let samples_since_peak = self
            .scratch_fine_vec
            .len()
            .saturating_sub(1)
            .saturating_sub(max_energy_index);
        let latency_seconds = samples_since_peak as f32 / self.fine_config.rate;
        let beat_offset = Some(Duration::from_secs_f32(latency_seconds));

        Ok(Some(AnalysisResult {
            bpm: smoothed_bpm,
            coarse_confidence: coarse_conf,
            is_drop,
            confidence,
            energy: norm_res_fine.energy_mean,
            average_energy: avg_history_energy,
            beat_offset,
        }))
    }
}
