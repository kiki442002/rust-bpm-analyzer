#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub mod button {
    use futures::stream::StreamExt;
    use gpio_cdev::{AsyncLineEventHandle, Chip, EventRequestFlags, LineRequestFlags};
    use std::time::Duration;
    use tokio::sync::mpsc::Sender;
    use tokio::time::{Instant, sleep_until};

    /// Les différents types d'actions détectées
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum ButtonAction {
        SinglePress,
        DoublePress,
        LongPress,
    }

    /// Tâche asynchrone qui écoute un GPIO
    pub struct ButtonListener {
        chip_path: String,
        line_offset: u32,
        debounce_ms: u64,
        double_press_ms: u64,
        long_press_ms: u64,
    }

    impl ButtonListener {
        pub fn new(chip_path: &str, line_offset: u32) -> Self {
            Self {
                chip_path: chip_path.to_string(),
                line_offset,
                debounce_ms: 60,
                double_press_ms: 300,
                long_press_ms: 800,
            }
        }

        pub fn with_timings(
            mut self,
            debounce_ms: u64,
            double_press_ms: u64,
            long_press_ms: u64,
        ) -> Self {
            self.debounce_ms = debounce_ms;
            self.double_press_ms = double_press_ms;
            self.long_press_ms = long_press_ms;
            self
        }

        /// Lance la boucle d'écoute. Cette fonction ne retourne pas (sauf erreur).
        pub async fn run(
            self,
            sender: Sender<ButtonAction>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let mut chip = Chip::new(&self.chip_path)?;
            let line = chip.get_line(self.line_offset)?;

            // On demande les événements sur les deux fronts (Appui et Relâchement)
            let handle = line.events(
                LineRequestFlags::INPUT,
                EventRequestFlags::BOTH_EDGES,
                "rust-bpm-button",
            )?;

            // Conversion en Stream Async via gpio-cdev features=["async-tokio"]
            let mut events = AsyncLineEventHandle::new(handle)?;

            let mut press_start_time: Option<Instant> = None;
            let mut click_count = 0;
            let mut long_press_sent = false;
            let mut last_event_time = Instant::now();

            // Timers "immobiles" (dans le passé ou futur très lointain)
            let far_future = Instant::now() + Duration::from_secs(365 * 24 * 3600);
            let double_click_timer = sleep_until(far_future);
            let long_press_timer = sleep_until(far_future);

            // On les épingle (Pin) pour pouvoir les utiliser dans select!
            tokio::pin!(double_click_timer);
            tokio::pin!(long_press_timer);

            println!(
                "Button Listener started on {} line {}",
                self.chip_path, self.line_offset
            );

            loop {
                tokio::select! {
                    // 1. Événement GPIO (via Stream)
                    Some(event_result) = events.next() => {
                        match event_result {
                            Ok(event) => {
                                let now = Instant::now();
                                if now.duration_since(last_event_time) < Duration::from_millis(self.debounce_ms) {
                                    continue;
                                }
                                last_event_time = now;

                                let is_pressed = event.event_type() == gpio_cdev::EventType::FallingEdge;

                                if is_pressed {
                                    press_start_time = Some(now);
                                    long_press_sent = false;
                                    long_press_timer.as_mut().reset(now + Duration::from_millis(self.long_press_ms));
                                } else {
                                    long_press_timer.as_mut().reset(far_future);
                                    if let Some(_start) = press_start_time {
                                        press_start_time = None;
                                        if long_press_sent {
                                            click_count = 0;
                                        } else {
                                            click_count += 1;
                                            double_click_timer.as_mut().reset(now + Duration::from_millis(self.double_press_ms));
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                eprintln!("Erreur GPIO Stream: {}", e);
                                // On peut décider de continuer ou break
                            }
                        }
                    }

                    // 2. Timeout Long Press
                    _ = &mut long_press_timer => {
                         if press_start_time.is_some() && !long_press_sent {
                             let _ = sender.send(ButtonAction::LongPress).await;
                             long_press_sent = true;
                             click_count = 0;
                         }
                         long_press_timer.as_mut().reset(far_future);
                    }

                    // 3. Timeout Double Click
                    _ = &mut double_click_timer => {
                        if click_count == 1 {
                             let _ = sender.send(ButtonAction::SinglePress).await;
                        } else if click_count >= 2 {
                             let _ = sender.send(ButtonAction::DoublePress).await;
                        }
                        click_count = 0;
                        double_click_timer.as_mut().reset(far_future);
                    }
                }
            }
        }
    }
}
