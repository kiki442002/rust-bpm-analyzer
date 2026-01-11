use alsa::Mixer;
use alsa::mixer::{Selem, SelemChannelId, SelemId};
use std::time::Instant;

pub struct AudioPID {
    kp: f32,
    ki: f32,
    kd: f32,
    prev_error: f32,
    integral: f32,
    output_min: i64,
    output_max: i64,
    last_update: Option<Instant>,
    selem: Selem,
}

impl AudioPID {
    /// Met à jour le PID à partir d'un buffer et applique le gain à ALSA
    /// `mixer_name` = "default" ou autre, `selem_name` = "Master" ou autre
    pub fn update_alsa_from_slice(&mut self, setpoint: f32, buffer: &[f32]) -> Result<i64, String> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let sum: f32 = buffer.iter().sum();
        let mean = sum / buffer.len() as f32;
        let gain = self.update(setpoint, mean)?;

        self.selem
            .set_playback_volume(SelemChannelId::FrontLeft, gain)
            .map_err(|e| format!("set_playback_volume Error: {}", e))?;
        Ok(gain)
    }
    /// Met à jour le PID à partir d'un slice de valeurs (ex: buffer audio), dt calculé automatiquement
    pub fn update_from_slice(&mut self, setpoint: f32, buffer: &[f32]) -> Result<i64, String> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let sum: f32 = buffer.iter().sum();
        let mean = sum / buffer.len() as f32;
        self.update(setpoint, mean)
    }
    pub fn new(kp: f32, ki: f32, kd: f32, mixer_name: &str) -> Result<Self, String> {
        let mixer =
            Mixer::new(mixer_name, false).map_err(|e| format!("Mixer::new Error: {}", e))?;
        // Recherche du premier selem playback dispo
        let mut found = None;
        for elem in mixer.iter() {
            if let Some(selem) = elem.get_selem() {
                if selem.has_capture_volume() {
                    let (output_min, output_max) = selem.get_capture_volume_range();
                    found = Some((selem, output_min, output_max));
                    break;
                }
            }
        }
        let (selem, output_min, output_max) =
            found.ok_or_else(|| "Aucun selem playback trouvé sur 'default'".to_string())?;
        Ok(AudioPID {
            kp,
            ki,
            kd,
            prev_error: 0.0,
            integral: 0.0,
            output_min,
            output_max,
            last_update: None,
            selem,
        })
    }

    pub fn reset(&mut self) {
        self.prev_error = 0.0;
        self.integral = 0.0;
        self.last_update = None;
    }

    /// Met à jour le PID avec dt calculé automatiquement
    pub fn update(&mut self, setpoint: f32, measured: f32) -> Result<i64, String> {
        let now = Instant::now();
        let dt = if let Some(last) = self.last_update {
            let secs = (now - last).as_secs_f32();
            if secs > 0.0 { secs } else { 1e-6 }
        } else {
            1e-3 // Valeur par défaut pour la première itération
        };
        self.last_update = Some(now);

        let error = setpoint - measured;
        self.integral += error * dt;
        let derivative = (error - self.prev_error) / dt;
        self.prev_error = error;

        let mut output = self.kp * error + self.ki * self.integral + self.kd * derivative;
        if output > self.output_max as f32 {
            output = self.output_max as f32;
        } else if output < self.output_min as f32 {
            output = self.output_min as f32;
        }
        Ok(output.round() as i64)
    }
}
