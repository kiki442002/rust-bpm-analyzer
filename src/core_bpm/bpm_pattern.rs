use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct BpmPattern {
    // [nb_bpm][lengh][32]
    pub data: Vec<Vec<Vec<i32>>>,
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
        let nb_bpm = ((end_bpm - start_bpm) / step).ceil() as usize;
        let mut data = Vec::with_capacity(nb_bpm);

        for i in 0..nb_bpm {
            let bpm = start_bpm + i as f32 * step;
            let mut bpm_patterns = Vec::with_capacity(lengh);
            for x in 0..lengh {
                let mut beat_positions = Vec::with_capacity(32);
                let interval = (60.0 / bpm) * frame_rate as f32;
                let mut timestamp = 0;
                for _ in 0..32 {
                    beat_positions.push(timestamp);
                    timestamp += interval as i32;
                }
                // Décalage éventuel (jump) si besoin
                let jump = 20 * (x as i32 + 1);
                let beat_positions: Vec<i32> = beat_positions.iter().map(|v| v + jump).collect();
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
}
