use biquad::*;
use std::collections::VecDeque;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
struct BpmHistoryEntry {
    bpm: f32,
    energy: f32,
    _timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct AnalysisResult {
    pub bpm: f32,
    pub is_drop: bool,
    pub confidence: f32,
    pub coarse_confidence: f32,
    pub energy: f32,
    pub average_energy: f32,
}

pub struct BpmAnalyzer {
    // Buffers
    coarse_buffer: VecDeque<f32>,
    fine_buffer: VecDeque<f32>,

    // Historique structuré (BPM, Energie, Temps)
    history: VecDeque<BpmHistoryEntry>,
    last_detection_time: Instant,

    // Rates
    coarse_rate: f32,
    fine_rate: f32,

    // Downsampling steps
    fine_step: usize,
    coarse_step: usize,

    // Filtres (Passe-Haut + Passe-Bas)
    filter1: DirectForm2Transposed<f32>,
    filter2: DirectForm2Transposed<f32>,

    // Seuils
    pub min_confidence: f32,
    pub min_coarse_confidence: f32,
    pub average_energy: f32, // Moyenne glissante de l'énergie (interne)

    // Reference BPM (Lock sur Drop)
    reference_bpm: f32,
}

impl BpmAnalyzer {
    pub fn new(sample_rate: u32) -> Result<Self, Box<dyn std::error::Error>> {
        // Stratégie Coarse-Fine
        // Fine Rate : ~11000 Hz (Compromis Précision/CPU)
        // Coarse Rate : ~500 Hz (Recherche rapide)

        // Pour 44100Hz : Step 4 => 11025 Hz. Pour 16000Hz : Step 2 => 8000 Hz.
        let fine_step = if sample_rate >= 44100 { 4 } else { 2 };

        // Pour garder ~500Hz en Coarse :
        // 11025 / 22 ~= 501 Hz.
        // 8000 / 16 = 500 Hz.
        let coarse_step = if sample_rate >= 44100 { 22 } else { 16 };

        let fine_rate = sample_rate as f32 / fine_step as f32;
        let coarse_rate = fine_rate / coarse_step as f32;

        let window_duration = 4.0; // 4 secondes d'historique

        let coarse_capacity = (coarse_rate * window_duration) as usize;
        let fine_capacity = (fine_rate * window_duration) as usize;

        // Configuration des filtres : BandPass 30Hz - 800Hz
        // Filter 1: HighPass 30Hz (Order 2) - Laisse passer les Sub-Bass (808, etc.)
        // Filter 2: LowPass 800Hz (Order 2) - Garde Kick/Snare, supprime Hi-Hats

        let hp_freq = 50.0;
        let lp_freq = 250.0;

        let hp_coeffs = Coefficients::<f32>::from_params(
            Type::HighPass,
            Hertz::<f32>::from_hz(sample_rate as f32).unwrap(),
            Hertz::<f32>::from_hz(hp_freq).unwrap(),
            Q_BUTTERWORTH_F32,
        )
        .map_err(|e| format!("Failed to create HP coefficients: {:?}", e))?;

        let lp_coeffs = Coefficients::<f32>::from_params(
            Type::LowPass,
            Hertz::<f32>::from_hz(sample_rate as f32).unwrap(),
            Hertz::<f32>::from_hz(lp_freq).unwrap(),
            Q_BUTTERWORTH_F32,
        )
        .map_err(|e| format!("Failed to create LP coefficients: {:?}", e))?;

        let filter1 = DirectForm2Transposed::<f32>::new(hp_coeffs);
        let filter2 = DirectForm2Transposed::<f32>::new(lp_coeffs);

        println!("BPM Analyzer Configured:");
        println!("  Sample Rate: {} Hz", sample_rate);
        println!("  Fine Rate: {:.2} Hz (Step {})", fine_rate, fine_step);
        println!(
            "  Coarse Rate: {:.2} Hz (Step {})",
            coarse_rate, coarse_step
        );

        Ok(Self {
            coarse_buffer: VecDeque::with_capacity(coarse_capacity),
            fine_buffer: VecDeque::with_capacity(fine_capacity),
            history: VecDeque::with_capacity(5),
            last_detection_time: Instant::now(),
            coarse_rate,
            fine_rate,
            fine_step,
            coarse_step,
            filter1,
            filter2,
            min_confidence: 0.3,
            min_coarse_confidence: 0.4,
            average_energy: 0.0, // Initialisation
            reference_bpm: 0.0,
        })
    }

    pub fn process(&mut self, new_samples: &[f32]) -> AnalysisResult {
        let empty_result = AnalysisResult {
            bpm: 0.0,
            is_drop: false,
            confidence: 0.0,
            coarse_confidence: 0.0,
            energy: 0.0,
            average_energy: self.average_energy,
        };

        // 1. Filtrage et Downsampling (Input -> Fine)
        let mut fine_chunk_accum = Vec::with_capacity(new_samples.len() / self.fine_step);

        for chunk in new_samples.chunks(self.fine_step) {
            let mut sum = 0.0;
            for &x in chunk {
                // Application du filtre d'ordre 4
                let y1 = self.filter1.run(x);
                let y2 = self.filter2.run(y1);
                sum += y2.abs(); // Rectification
            }
            let avg = sum / chunk.len() as f32;
            fine_chunk_accum.push(avg);
        }

        // Mise à jour du buffer Fine
        for &sample in &fine_chunk_accum {
            if self.fine_buffer.len() >= self.fine_buffer.capacity() {
                self.fine_buffer.pop_front();
            }
            self.fine_buffer.push_back(sample);
        }

        // 2. Downsampling (Fine -> Coarse)
        // On traite uniquement les nouveaux échantillons Fine
        for chunk in fine_chunk_accum.chunks(self.coarse_step) {
            let sum: f32 = chunk.iter().sum();
            let avg = sum / chunk.len() as f32;

            if self.coarse_buffer.len() >= self.coarse_buffer.capacity() {
                self.coarse_buffer.pop_front();
            }
            self.coarse_buffer.push_back(avg);
        }

        // Il nous faut au moins 2 secondes de données
        if self.coarse_buffer.len() < (self.coarse_rate * 2.0) as usize {
            return empty_result;
        }

        // ============================================================
        // ÉTAPE 1 : RECHERCHE GROSSIÈRE (COARSE)
        // ============================================================

        let mut coarse_vec: Vec<f32> = self.coarse_buffer.iter().cloned().collect();

        // Normalisation de la fenêtre Coarse (0..1)
        let max_c = coarse_vec.iter().cloned().fold(0.0 / 0.0, f32::max);
        if max_c > 0.0 {
            for x in &mut coarse_vec {
                *x /= max_c;
            }
        }

        let coarse_mean: f32 = coarse_vec.iter().sum::<f32>() / coarse_vec.len() as f32;
        let coarse_centered: Vec<f32> = coarse_vec.iter().map(|x| x - coarse_mean).collect();
        let coarse_energy: f32 = coarse_centered.iter().map(|x| x * x).sum();

        // Plage de recherche : 60 à 310 BPM
        let min_bpm = 60.0;
        let max_bpm = 310.0;

        let min_lag_c = (self.coarse_rate * 60.0 / max_bpm) as usize;
        let max_lag_c = (self.coarse_rate * 60.0 / min_bpm) as usize;

        let mut best_lag_c = 0;
        let mut max_corr_c = 0.0;

        for lag in min_lag_c..=max_lag_c {
            let mut corr = 0.0;
            for i in 0..(coarse_centered.len() - lag) {
                corr += coarse_centered[i] * coarse_centered[i + lag];
            }
            if corr > max_corr_c {
                max_corr_c = corr;
                best_lag_c = lag;
            }
        }

        if best_lag_c == 0 {
            return empty_result;
        }

        let mut coarse_conf = 0.0;

        // Vérification de la confiance Coarse (Permissive)
        if coarse_energy > 0.001 {
            coarse_conf = max_corr_c / coarse_energy;
            if coarse_conf < self.min_coarse_confidence {
                // Signal trop faible ou trop bruité même pour une recherche grossière
                return empty_result;
            }
        } else {
            return empty_result; // Silence
        }

        // Correction d'octave (Harmonic Check)
        // On vérifie les harmoniques rapides (2x et 3x BPM)
        // On privilégie toujours le tempo le plus rapide si une corrélation significative existe.

        let initial_lag = best_lag_c;
        let initial_corr = max_corr_c;

        // 1. Check 2x BPM (Half Lag)
        let half_lag = initial_lag / 2;
        if half_lag >= min_lag_c {
            let mut max_half_corr = 0.0;
            let mut best_half_lag = 0;

            for lag in (half_lag.saturating_sub(1))..=(half_lag + 1) {
                let mut corr = 0.0;
                for i in 0..(coarse_centered.len() - lag) {
                    corr += coarse_centered[i] * coarse_centered[i + lag];
                }
                if corr > max_half_corr {
                    max_half_corr = corr;
                    best_half_lag = lag;
                }
            }

            // Seuil abaissé à 40% : Si l'harmonique 2x est présente, on la prend.
            if max_half_corr > (initial_corr * 0.4) {
                best_lag_c = best_half_lag;
                // On met à jour max_corr_c pour que le check 3x puisse comparer avec le "gagnant" actuel si besoin
                // Mais ici on compare toujours à l'initial pour le seuil relatif
            }
        }

        // 2. Check 3x BPM (Third Lag) - Ex: 66.7 -> 200 BPM
        let third_lag = initial_lag / 3;
        if third_lag >= min_lag_c {
            let mut max_third_corr = 0.0;
            let mut best_third_lag = 0;

            for lag in (third_lag.saturating_sub(1))..=(third_lag + 1) {
                let mut corr = 0.0;
                for i in 0..(coarse_centered.len() - lag) {
                    corr += coarse_centered[i] * coarse_centered[i + lag];
                }
                if corr > max_third_corr {
                    max_third_corr = corr;
                    best_third_lag = lag;
                }
            }

            // Seuil abaissé à 30% : Très agressif pour récupérer les tempos rapides (DnB/Hardcore)
            // Si on a trouvé un candidat 3x valide, il écrase le candidat 2x ou 1x.
            if max_third_corr > (initial_corr * 0.3) {
                best_lag_c = best_third_lag;
            }
        }
        // ============================================================
        // ÉTAPE 2 : RAFFINEMENT (FINE)
        // ============================================================

        // Conversion du Lag Coarse vers Fine
        // Ratio = fine_rate / coarse_rate = coarse_step
        let center_lag_f = best_lag_c * self.coarse_step;

        // Fenêtre de recherche Fine (+/- 50 samples pour couvrir la marge d'erreur avec le nouveau taux)
        let search_radius = 50;
        let min_lag_f = center_lag_f.saturating_sub(search_radius);
        let max_lag_f = center_lag_f + search_radius;

        let mut fine_vec: Vec<f32> = self.fine_buffer.iter().cloned().collect();

        // Normalisation de la fenêtre Fine (0..1)
        // On garde le max absolu pour le Noise Gate
        let raw_max_f = fine_vec.iter().cloned().fold(0.0 / 0.0, f32::max);

        if raw_max_f > 0.0 {
            for x in &mut fine_vec {
                *x /= raw_max_f;
            }
        }

        let fine_mean: f32 = fine_vec.iter().sum::<f32>() / fine_vec.len() as f32;
        let fine_centered: Vec<f32> = fine_vec.iter().map(|x| x - fine_mean).collect();
        let fine_energy_sum: f32 = fine_centered.iter().map(|x| x * x).sum();
        let fine_energy_mean = if !fine_centered.is_empty() {
            fine_energy_sum / fine_centered.len() as f32
        } else {
            0.0
        };

        // Noise Gate : Si le signal brut est trop faible (< 1% du volume max possible), on ignore
        if raw_max_f < 0.01 {
            return empty_result;
        }

        // Seuil d'énergie minimale (basé sur la moyenne maintenant)
        // 0.00001 est un seuil raisonnable pour une variance moyenne normalisée
        if fine_energy_mean < 0.00001 {
            return empty_result;
        }

        let mut best_lag_f = 0;
        let mut max_corr_f = 0.0;

        // On s'assure de rester dans les bornes du buffer
        let safe_max_lag = fine_centered.len().saturating_sub(1);
        let start_lag = min_lag_f.max(1);
        let end_lag = max_lag_f.min(safe_max_lag);

        for lag in start_lag..=end_lag {
            let mut corr = 0.0;
            for i in 0..(fine_centered.len() - lag) {
                corr += fine_centered[i] * fine_centered[i + lag];
            }
            if corr > max_corr_f {
                max_corr_f = corr;
                best_lag_f = lag;
            }
        }

        if best_lag_f == 0 {
            return empty_result;
        }

        // Vérification de la confiance sur le signal Fine
        // Confidence = Covariance / Variance (approximatif ici car lags différents, mais suffisant)
        let confidence = if fine_energy_sum > 0.0 {
            max_corr_f / fine_energy_sum
        } else {
            0.0
        };

        if confidence < self.min_confidence {
            return empty_result;
        }

        // ============================================================
        // ÉTAPE 3 : INTERPOLATION PARABOLIQUE
        // ============================================================

        let mut refined_lag = best_lag_f as f32;

        if best_lag_f > start_lag && best_lag_f < end_lag {
            let calc_corr = |l: usize| -> f32 {
                let mut c = 0.0;
                for i in 0..(fine_centered.len() - l) {
                    c += fine_centered[i] * fine_centered[i + l];
                }
                c
            };

            let y_prev = calc_corr(best_lag_f - 1);
            let y_curr = max_corr_f;
            let y_next = calc_corr(best_lag_f + 1);

            let denominator = 2.0 * (y_prev - 2.0 * y_curr + y_next);
            if denominator.abs() > 0.0001 {
                let offset = (y_prev - y_next) / denominator;
                refined_lag = best_lag_f as f32 + offset;
            }
        }

        // Calcul final du BPM
        let bpm = (self.fine_rate * 60.0) / refined_lag;

        // Arrondi à 0.1 près
        let raw_bpm = (bpm * 10.0).round() / 10.0;

        // ============================================================
        // DÉTECTION DE DROP (AMÉLIORÉE - Comparaison Intra-Fenêtre)
        // ============================================================
        // On calcule le Drop AVANT de valider le BPM pour l'historique

        let split_index = (fine_vec.len() * 3) / 4; // 75% du buffer

        // 1. Énergie de l'historique (0..75%)
        let mut history_sum_sq = 0.0;
        for i in 0..split_index {
            let val = fine_vec[i];
            history_sum_sq += val * val;
        }
        let history_count = split_index.max(1);
        let history_energy = history_sum_sq / history_count as f32;

        // 2. Énergie récente (75%..100%)
        let mut recent_sum_sq = 0.0;
        for i in split_index..fine_vec.len() {
            let val = fine_vec[i];
            recent_sum_sq += val * val;
        }
        let recent_count = (fine_vec.len() - split_index).max(1);
        let current_energy = recent_sum_sq / recent_count as f32;

        // 3. Détection
        let is_drop = (current_energy > history_energy * 1.2) && (current_energy > 0.01);

        // ============================================================
        // GESTION DE L'HISTORIQUE ET LISSAGE
        // ============================================================

        let now = Instant::now();

        // 1. Reset si silence prolongé (> 5s)
        if now.duration_since(self.last_detection_time).as_secs_f32() > 10.0 {
            self.history.clear();
            self.reference_bpm = 0.0;
        }

        // 2. Calcul de l'énergie moyenne actuelle de l'historique (pour le seuil)
        let avg_history_energy = if self.history.is_empty() {
            0.0
        } else {
            self.history.iter().map(|e| e.energy).sum::<f32>() / self.history.len() as f32
        };

        // 3. Adaptive Energy Threshold (Gate)
        if !self.history.is_empty()
            && fine_energy_mean < (avg_history_energy * 0.9)
            && fine_energy_mean < 0.03
        {
            return empty_result;
        }

        // 4. Filtrage par Référence (Lock sur Drop)
        if is_drop {
            // Si c'est un Drop, on met à jour la référence
            self.reference_bpm = raw_bpm;
        } else if self.reference_bpm > 0.0 {
            // Si ce n'est pas un Drop mais qu'on a une référence, on vérifie la cohérence
            let test_ref = self.reference_bpm * 0.1;
            let is_close = (raw_bpm - self.reference_bpm).abs() <= test_ref;

            // Vérification des harmoniques (x2, /2, x3)
            let is_double = (raw_bpm - self.reference_bpm * 2.0).abs() <= test_ref / 2.0;
            let is_half = (raw_bpm - self.reference_bpm / 2.0).abs() <= test_ref * 2.0;
            let is_triple = (raw_bpm - self.reference_bpm * 3.0).abs() <= test_ref / 3.0;

            if !is_close && !is_double && !is_half && !is_triple {
                // BPM incohérent avec la référence -> On ignore cette détection
                return empty_result;
            }
        } else {
            // Pas de référence encore, on peut l'accepter
            return empty_result;
        }

        // 5. Mise à jour de l'historique
        if self.history.len() >= 5 {
            self.history.pop_front();
        }
        self.history.push_back(BpmHistoryEntry {
            bpm: raw_bpm,
            energy: fine_energy_mean,
            _timestamp: now,
        });
        self.last_detection_time = now;

        // 6. Calcul des valeurs lissées
        // Median BPM
        let mut sorted_bpm: Vec<f32> = self.history.iter().map(|e| e.bpm).collect();
        sorted_bpm.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let smoothed_bpm = if !sorted_bpm.is_empty() {
            sorted_bpm[sorted_bpm.len() / 2]
        } else {
            raw_bpm
        };

        // Mean Energy
        let smoothed_energy = if !self.history.is_empty() {
            self.history.iter().map(|e| e.energy).sum::<f32>() / self.history.len() as f32
        } else {
            fine_energy_mean
        };

        // On met à jour la moyenne persistante juste pour info
        self.average_energy = smoothed_energy;

        AnalysisResult {
            bpm: smoothed_bpm,
            coarse_confidence: coarse_conf,
            is_drop,
            confidence,
            energy: fine_energy_mean,
            average_energy: smoothed_energy,
        }
    }
}
