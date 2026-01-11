use alsa::Mixer;
use alsa::mixer::{SelemChannelId, SelemId};
use std::time::Instant;

pub struct AudioPID {
    kp: f32,
    ki: f32,
    kd: f32,
    prev_error: f32,
    integral: f32,
    output_min: f32,
    output_max: f32,
    last_update: Option<Instant>,
}

impl AudioPID {
    /// Met à jour le PID à partir d'un buffer et applique le gain à ALSA
    /// `mixer_name` = "default" ou autre, `selem_name` = "Master" ou autre
    pub fn update_alsa_from_slice(
        &mut self,
        setpoint: f32,
        buffer: &[f32],
        mixer_name: &str,
        selem_name: &str,
        channel: SelemChannelId,
    ) -> Result<f32> {
        if buffer.is_empty() {
            return Ok(0.0);
        }
        let sum: f32 = buffer.iter().sum();
        let mean = sum / buffer.len() as f32;
        let gain = self.update(setpoint, mean)?;

        let mixer = Mixer::new(mixer_name, false)?;
        let sid = SelemId::new(selem_name, 0);
        let selem = match mixer.find_selem(&sid) {
            Some(s) => s,
            None => return Err("find_selem Error".into()),
        };
        let (min, max) = selem.get_playback_volume_range();
        // Map le gain PID (output_min/output_max) sur la plage ALSA
        let alsa_gain = min as f32
            + (gain - self.output_min) * (max - min) as f32 / (self.output_max - self.output_min);
        selem.set_playback_volume(channel, alsa_gain as i64)?;
        Ok(gain)
    }
    /// Met à jour le PID à partir d'un slice de valeurs (ex: buffer audio), dt calculé automatiquement
    pub fn update_from_slice(&mut self, setpoint: f32, buffer: &[f32]) -> Result<f32, String> {
        if buffer.is_empty() {
            return Ok(0.0 as f32);
        }
        let sum: f32 = buffer.iter().sum();
        let mean = sum / buffer.len() as f32;
        self.update(setpoint, mean)
    }
    pub fn new(kp: f32, ki: f32, kd: f32, output_min: f32, output_max: f32) -> Self {
        AudioPID {
            kp,
            ki,
            kd,
            prev_error: 0.0,
            integral: 0.0,
            output_min,
            output_max,
            last_update: None,
        }
    }

    pub fn reset(&mut self) {
        self.prev_error = 0.0;
        self.integral = 0.0;
        self.last_update = None;
    }

    /// Met à jour le PID avec dt calculé automatiquement
    pub fn update(&mut self, setpoint: f32, measured: f32) -> Result<f32, String> {
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
        if output > self.output_max {
            output = self.output_max;
        } else if output < self.output_min {
            output = self.output_min;
        }
        Ok(output)
    }
}
