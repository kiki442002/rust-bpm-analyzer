// Charge le pattern BPM embarqué dans le binaire
pub fn load_embedded_pattern() -> (BpmPattern, BpmPattern) {
    let bytes_coarse = include_bytes!("../../assets/BPM_pattern_coarse.bin");
    let pattern_coarse =
        postcard::from_bytes(bytes_coarse).expect("Erreur de désérialisation du pattern");

    let bytes_fine = include_bytes!("../../assets/BPM_pattern_fine.bin");
    let pattern_fine =
        postcard::from_bytes(bytes_fine).expect("Erreur de désérialisation du pattern");

    (pattern_coarse, pattern_fine)
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct BpmPattern {
    // [nb_bpm][lengh][32]
    pub data: Vec<Vec<Vec<u32>>>,
    pub start_bpm: f32,
    pub step: f32,
    pub lengh: usize,
    pub frame_rate: usize,
}

impl BpmPattern {
    pub fn generate(
        start_bpm: f32,
        end_bpm: f32,
        step: f32,
        lengh: usize,
        frame_rate: usize,
    ) -> Self {
        // Pour matcher Python :
        // bpm = start_bpm - 10 + add ; add += step à chaque i
        let width = end_bpm - start_bpm;
        let sample = ((width + 10.0) / step).ceil() as usize;
        let mut data = Vec::with_capacity(sample);
        let mut add = 0.0f32;
        for _i in 0..sample {
            add += step;
            let bpm = start_bpm - 10.0 + add;
            let mut bpm_patterns = Vec::with_capacity(lengh);
            let timestamp = (60.0 / bpm * frame_rate as f32) as u32;
            let mut jump = 0u32;
            for _x in 0..lengh {
                let mut beat_positions = Vec::with_capacity(32);
                let mut timestamp_next = 0u32;
                jump += 20;
                for _y in 0..32 {
                    beat_positions.push(timestamp_next);
                    timestamp_next = timestamp_next.saturating_add(timestamp);
                }
                let beat_positions: Vec<u32> = beat_positions
                    .iter()
                    .map(|v| v.saturating_add(jump))
                    .collect();
                bpm_patterns.push(beat_positions);
            }
            data.push(bpm_patterns);
        }
        BpmPattern {
            data,
            start_bpm,
            step,
            lengh,
            frame_rate,
        }
    }

    /// Génère et sauvegarde le pattern dans un fichier binaire (postcard)
    pub fn generate_and_save(
        start_bpm: f32,
        end_bpm: f32,
        step: f32,
        lengh: usize,
        frame_rate: usize,
        path: &str,
    ) -> std::io::Result<()> {
        let pattern = Self::generate(start_bpm, end_bpm, step, lengh, frame_rate);
        let encoded: Vec<u8> =
            postcard::to_allocvec(&pattern).expect("Erreur de sérialisation du pattern");
        std::fs::write(path, &encoded)
    }
}
