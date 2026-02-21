#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod button {
    use futures::stream::StreamExt;
    use gpio_cdev::{AsyncLineEventHandle, Chip, EventRequestFlags, EventType, LineRequestFlags};
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc;
    use tokio::task;

    #[derive(Debug, Clone, PartialEq)]
    pub enum ButtonEvent {
        Press,         // < 2.5s
        LongPress,     // >= 2.5s and < 7.5s
        VeryLongPress, // >= 7.5s
    }

    pub struct Button {
        // We defer creation of the AsyncLineEventHandle until monitoring starts
        // to avoid Send/Sync issues with the handle across threads before spawning.
        // We store config parameters.
        gpio_chip: String,
        line_offset: u32,
        active_low: bool,
    }

    impl Button {
        pub fn new(gpio_chip: &str, line_offset: u32, active_low: bool) -> Self {
            Self {
                gpio_chip: gpio_chip.to_string(),
                line_offset,
                active_low,
            }
        }

        pub fn monitor(self) -> mpsc::Receiver<ButtonEvent> {
            let (tx, rx) = mpsc::channel(10);
            let active_low = self.active_low;
            let gpio_chip = self.gpio_chip.clone();
            let line_offset = self.line_offset;

            task::spawn(async move {
                // Initialize GPIO inside the task
                let mut chip = match Chip::new(&gpio_chip) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Failed to open GPIO chip: {}", e);
                        return;
                    }
                };

                let line = match chip.get_line(line_offset) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Failed to get GPIO line: {}", e);
                        return;
                    }
                };

                let event_handle = match line.events(
                    LineRequestFlags::INPUT,
                    EventRequestFlags::BOTH_EDGES,
                    "button_monitor",
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to request line events: {}", e);
                        return;
                    }
                };

                // Create Async handle
                let mut events = match AsyncLineEventHandle::new(event_handle) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to create AsyncLineEventHandle: {}", e);
                        return;
                    }
                };

                let mut press_start: Option<Instant> = None;

                while let Some(event_result) = events.next().await {
                    match event_result {
                        Ok(event) => {
                            let is_pressed = if active_low {
                                // Active Low (Pull Up): Press = Falling Edge (High -> Low)
                                match event.event_type() {
                                    EventType::FallingEdge => true,
                                    EventType::RisingEdge => false,
                                }
                            } else {
                                // Active High (Pull Down): Press = Rising Edge (Low -> High)
                                match event.event_type() {
                                    EventType::RisingEdge => true,
                                    EventType::FallingEdge => false,
                                }
                            };

                            if is_pressed {
                                press_start = Some(Instant::now());
                            } else {
                                if let Some(start) = press_start {
                                    let duration = start.elapsed();
                                    press_start = None;

                                    let event_type = if duration >= Duration::from_millis(7500) {
                                        Some(ButtonEvent::VeryLongPress)
                                    } else if duration >= Duration::from_millis(2500) {
                                        Some(ButtonEvent::LongPress)
                                    } else if duration >= Duration::from_millis(50) {
                                        Some(ButtonEvent::Press)
                                    } else {
                                        None
                                    };

                                    if let Some(evt) = event_type {
                                        if tx.send(evt).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Button monitor error: {}", e);
                        }
                    }
                }
            });
            rx
        }
    }
}
