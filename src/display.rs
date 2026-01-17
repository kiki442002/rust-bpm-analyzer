#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod display {
    use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_6X10};
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::Text;
    use linux_embedded_hal::I2cdev;
    use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};

    pub struct BpmDisplay {
        display: Ssd1306<
            I2CInterface<I2cdev>,
            DisplaySize128x64,
            BufferedGraphicsMode<DisplaySize128x64>,
        >,
    }

    impl BpmDisplay {
        pub fn new(i2c_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
            let i2c = I2cdev::new(i2c_path)?;
            let interface = I2CDisplayInterface::new(i2c);
            let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode();
            display.init()?;
            display.clear();
            display.flush()?;
            Ok(BpmDisplay { display })
        }

        pub fn show_bpm(&mut self, bpm: f32) -> Result<(), Box<dyn std::error::Error>> {
            self.display.clear();
            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            let text = format!("BPM: {:.2}", bpm);
            Text::new(&text, Point::new(0, 16), style).draw(&mut self.display)?;
            self.display.flush()?;
            Ok(())
        }
    }
}
