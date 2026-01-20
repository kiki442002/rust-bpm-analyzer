#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod led {
    use gpio_cdev::{Chip, LineHandle, LineRequestFlags};
    use std::time::Duration;
    use tokio::task;
    use tokio::time::{Duration, sleep};

    use std::sync::Arc;

    pub struct Led {
        handle: LineHandle,
    }

    impl Led {
        /// Initialise une LED sur le GPIO donné (ex: "/dev/gpiochip0", offset GPIO)
        pub fn new(gpio_chip: &str, line_offset: u32) -> Result<Self, Box<dyn std::error::Error>> {
            let mut chip = Chip::new(gpio_chip)?;
            let handle =
                chip.get_line(line_offset)?
                    .request(LineRequestFlags::OUTPUT, 0, "led_control")?;
            Ok(Led { handle })
        }

        /// Allume la LED
        pub fn on(&self) -> Result<(), Box<dyn std::error::Error>> {
            self.handle.set_value(1)?;
            Ok(())
        }

        /// Éteint la LED
        pub fn off(&self) -> Result<(), Box<dyn std::error::Error>> {
            self.handle.set_value(0)?;
            Ok(())
        }

        /// Fait clignoter la LED n fois avec un délai en ms (async)
        pub async fn blink(
            &self,
            times: u32,
            delay_ms: u64,
        ) -> Result<(), Box<dyn std::error::Error>> {
            for _ in 0..times {
                self.on()?;
                sleep(Duration::from_millis(delay_ms)).await;
                self.off()?;
                sleep(Duration::from_millis(delay_ms)).await;
            }
            Ok(())
        }

        /// Fait clignoter la LED dans une tâche tokio (non bloquant)
        pub fn blink_async(self: Arc<Self>, times: u32, delay_ms: u64) {
            task::spawn(async move {
                for _ in 0..times {
                    let _ = self.on();
                    sleep(Duration::from_millis(delay_ms)).await;
                    let _ = self.off();
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            });
        }
    }
}
