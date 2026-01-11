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
    selem_id: SelemId,
}

impl AudioPID {
    /// Met à jour le PID à partir d'un buffer et applique le gain à ALSA
    /// Utilise le taux de clipping comme mesure pour le PID
    /// `clip_threshold` : seuil à partir duquel un échantillon est considéré comme "clippé"
    pub fn update_alsa_from_slice(
        &mut self,
        setpoint: f32,
        buffer: &[f32],
        mixer: &alsa::Mixer,
        clip_threshold: f32,
    ) -> Result<i64, String> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let clipped = buffer
            .iter()
            .filter(|&&x| x.abs() >= clip_threshold)
            .count();
        let ratio = clipped as f32 / buffer.len() as f32;
        print!("Clip ratio: {:.4} | ", ratio);
        let gain = self.update(setpoint, ratio)?;

        let selem = mixer
            .find_selem(&self.selem_id)
            .ok_or_else(|| "Impossible de retrouver le contrôle audio".to_string())?;

        selem
            .set_capture_volume(SelemChannelId::FrontLeft, gain)
            .map_err(|e| format!("set_capture_volume Error: {}", e))?;
        Ok(gain)
    }

    /// Met à jour le PID à partir d'un buffer, retourne le gain calculé (utilise le taux de clipping)
    pub fn update_from_slice(
        &mut self,
        setpoint: f32,
        buffer: &[f32],
        clip_threshold: f32,
    ) -> Result<i64, String> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let clipped = buffer
            .iter()
            .filter(|&&x| x.abs() >= clip_threshold)
            .count();
        let ratio = clipped as f32 / buffer.len() as f32;
        self.update(setpoint, ratio)
    }

    pub fn new(kp: f32, ki: f32, kd: f32, mixer: &alsa::Mixer) -> Result<Self, String> {
        let mut found = None;
        for elem in mixer.iter() {
            // On tente de créer un Selem à partir de l'élément
            if let Some(selem) = Selem::new(elem) {
                if selem.has_capture_volume() {
                    let (min, max) = selem.get_capture_volume_range();
                    let id = selem.get_id();
                    found = Some((id, min, max));
                    break; // On a trouvé notre bonheur
                }
            }
        }
        let (selem_id, output_min, output_max) =
            found.ok_or_else(|| "No capture Selem found in mixer".to_string())?;

        println!(
            "AudioPID initialized | Capture Volume Range: {} - {}",
            output_min, output_max
        );
        Ok(AudioPID {
            kp,
            ki,
            kd,
            prev_error: 0.0,
            integral: 0.0,
            output_min,
            output_max,
            last_update: None,
            selem_id,
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
