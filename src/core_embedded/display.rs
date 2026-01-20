#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub mod display {
    use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_6X10};
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::Text;
    use linux_embedded_hal::I2cdev;
    use ssd1306::mode::BufferedGraphicsMode;
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
            // Tentative adresse par défaut (0x3C)
            eprintln!(
                "Tentative connexion OLED sur {} à l'adresse 0x3C...",
                i2c_path
            );
            match Self::try_init(i2c_path, 0x3C) {
                Ok(display) => return Ok(display),
                Err(e) => eprintln!("-> Échec 0x3C: {:?}", e),
            }

            // Tentative adresse alternative (0x3D)
            eprintln!(
                "Tentative connexion OLED sur {} à l'adresse 0x3D...",
                i2c_path
            );
            Self::try_init(i2c_path, 0x3D)
        }

        fn try_init(i2c_path: &str, address: u8) -> Result<Self, Box<dyn std::error::Error>> {
            let i2c = I2cdev::new(i2c_path)?;
            let interface = I2CDisplayInterface::new_custom_address(i2c, address);
            let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode();

            display.init().map_err(|e| format!("Init error: {:?}", e))?;
            display
                .clear(BinaryColor::Off)
                .map_err(|e| format!("Clear error: {:?}", e))?;
            display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;
            Ok(BpmDisplay { display })
        }

        pub fn show_bpm(&mut self, bpm: f32) -> Result<(), Box<dyn std::error::Error>> {
            self.display
                .clear(BinaryColor::Off)
                .map_err(|e| format!("Clear error: {:?}", e))?;
            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            let text = format!("BPM: {:.2}", bpm);
            Text::new(&text, Point::new(0, 16), style)
                .draw(&mut self.display)
                .map_err(|e| format!("Draw error: {:?}", e))?;
            self.display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;
            Ok(())
        }
    }
}
