#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod pid_audio {
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
        rms_window: usize,
        rms_history: Vec<f32>,
    }

    impl AudioPID {
        /// Met à jour le PID à partir d'un buffer et applique le gain à ALSA
        /// `mixer_name` = "default" ou autre, `selem_name` = "Master" ou autre
        pub fn update_alsa_from_slice(
            &mut self,
            setpoint: f32,
            buffer: &[f32],
            mixer: &alsa::Mixer,
        ) -> Result<i64, String> {
            if buffer.is_empty() {
                return Ok(0);
            }
            let rms = (buffer.iter().map(|x| x * x).sum::<f32>() / buffer.len() as f32).sqrt();
            // Ajout à l'historique
            self.rms_history.push(rms);
            if self.rms_history.len() > self.rms_window {
                self.rms_history.remove(0);
            }
            let avg_rms = self.rms_history.iter().sum::<f32>() / self.rms_history.len() as f32;
            print!("Mean RMS: {:.4} | Smoothed RMS: {:.4} | ", rms, avg_rms);
            let gain = self.update(setpoint, avg_rms)?;

            let selem = mixer
                .find_selem(&self.selem_id)
                .ok_or_else(|| "Impossible de retrouver le contrôle audio".to_string())?;

            selem
                .set_capture_volume(SelemChannelId::FrontLeft, gain)
                .map_err(|e| format!("set_capture_volume Error: {}", e))?;
            Ok(gain)
        }

        pub fn new(
            kp: f32,
            ki: f32,
            kd: f32,
            rms_window: usize,
            mixer: &alsa::Mixer,
        ) -> Result<Self, String> {
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

            // Configure le volume au milieu de la plage
            let mid = (output_min + output_max) / 2;
            if let Some(selem) = mixer.find_selem(&selem_id) {
                let _ = selem.set_capture_volume(SelemChannelId::FrontLeft, mid);
            }

            println!(
                "AudioPID initialized | Capture Volume Range: {} - {} | Volume set to middle: {}",
                output_min, output_max, mid
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
                rms_window,
                rms_history: Vec::with_capacity(rms_window),
            })
        }

        pub fn reset(&mut self) {
            self.prev_error = 0.0;
            self.integral = 0.0;
            self.last_update = None;
            self.rms_history.clear();
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
}
