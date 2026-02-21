#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub mod display {
    use embedded_graphics::image::Image;
    use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_10X20};
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::Text;
    use linux_embedded_hal::I2cdev;
    use ssd1306::mode::BufferedGraphicsMode;
    use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};
    use tinybmp::Bmp;

    mod assets {
        pub const ICON_USB: &[u8] = include_bytes!("../../assets/display_asset/USB-tiny.bmp");
        pub const ICON_ETHERNET: &[u8] =
            include_bytes!("../../assets/display_asset/ethernet-tiny.bmp");
        pub const ICON_ETHERNET_INTERNET: &[u8] =
            include_bytes!("../../assets/display_asset/ethernet+internet-tiny.bmp");
        pub const ICON_UPDATE: &[u8] = include_bytes!("../../assets/display_asset/update-tiny.bmp");
    }

    /// Icônes disponibles pour la barre de statut
    pub enum StatusBarIcon {
        Usb,
        Ethernet,
        Internet,
        Update,
    }

    pub struct Icons {
        pub usb: Bmp<'static, BinaryColor>,
        pub ethernet: Bmp<'static, BinaryColor>,
        pub ethernet_internet: Bmp<'static, BinaryColor>,
        pub update: Bmp<'static, BinaryColor>,
    }

    impl Icons {
        pub fn new() -> Result<Self, String> {
            Ok(Self {
                usb: Bmp::from_slice(assets::ICON_USB).map_err(|e| format!("{:?}", e))?,
                ethernet: Bmp::from_slice(assets::ICON_ETHERNET).map_err(|e| format!("{:?}", e))?,
                ethernet_internet: Bmp::from_slice(assets::ICON_ETHERNET_INTERNET)
                    .map_err(|e| format!("{:?}", e))?,
                update: Bmp::from_slice(assets::ICON_UPDATE).map_err(|e| format!("{:?}", e))?,
            })
        }
    }

    pub struct BpmDisplay {
        display: Ssd1306<
            I2CInterface<I2cdev>,
            DisplaySize128x64,
            BufferedGraphicsMode<DisplaySize128x64>,
        >,
        icons: Icons,
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
            Err("Échec de l'initialisation de l'écran OLED".into())
        }

        /// Met à jour (flush) l'affichage
        pub fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            self.display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;
            Ok(())
        }

        /// Affiche une icône spécifique de la barre de statut
        pub fn draw_status_icon(
            &mut self,
            icon: StatusBarIcon,
        ) -> Result<(), Box<dyn std::error::Error>> {
            match icon {
                StatusBarIcon::Usb => {
                    Image::new(&self.icons.usb, Point::new(16, 8))
                        .draw(&mut self.display)
                        .map_err(|e| format!("{:?}", e))?;
                }
                StatusBarIcon::Ethernet => {
                    Image::new(&self.icons.ethernet, Point::new(48, 8))
                        .draw(&mut self.display)
                        .map_err(|e| format!("{:?}", e))?;
                }
                StatusBarIcon::Internet => {
                    Image::new(&self.icons.ethernet_internet, Point::new(48, 8))
                        .draw(&mut self.display)
                        .map_err(|e| format!("{:?}", e))?;
                }
                StatusBarIcon::Update => {
                    Image::new(&self.icons.update, Point::new(112, 8))
                        .draw(&mut self.display)
                        .map_err(|e| format!("{:?}", e))?;
                }
            }
            Ok(())
        }

        /// Efface une zone correspondant à une icône de la barre de statut
        pub fn clear_status_icon(
            &mut self,
            icon: StatusBarIcon,
        ) -> Result<(), Box<dyn std::error::Error>> {
            // Taille standard des icônes à effacer (ex: 16x16 par sécurité, ou la taille réelle des BMP)
            // Adaptez les dimensions (Size::new(w, h)) selon vos BMPs
            let size = Size::new(16, 16);

            let point = match icon {
                StatusBarIcon::Usb => Point::new(16, 8),
                StatusBarIcon::Ethernet | StatusBarIcon::Internet => Point::new(48, 8),
                StatusBarIcon::Update => Point::new(112, 8),
            };

            // Dessine un rectangle noir (Off) par dessus
            embedded_graphics::primitives::Rectangle::new(point, size)
                .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
                    BinaryColor::Off,
                ))
                .draw(&mut self.display)
                .map_err(|e| format!("{:?}", e))?;

            Ok(())
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

            // Affichage de démarrage
            let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            Text::new("***.**", Point::new(35, 45), style)
                .draw(&mut display)
                .map_err(|e| format!("Draw Hello error: {:?}", e))?;

            embedded_graphics::primitives::Rectangle::new(Point::new(1, 54), Size::new(127, 10))
                .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_stroke(
                    BinaryColor::On,
                    1,
                ))
                .draw(&mut display)
                .map_err(|e| format!("Rect audio error: {:?}", e))?;
            println!("OLED initialized at I2C address 0x{:02X}", address);

            display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;

            let icons = Icons::new().map_err(|e| format!("Icon load error: {}", e))?;
            Ok(BpmDisplay { display, icons })
        }

        pub fn show_bpm(&mut self, bpm: Option<f32>) -> Result<(), Box<dyn std::error::Error>> {
            // On efface la zone où le BPM est affiché pour éviter la superposition
            // Position (35, 45), Font 10x20. approx 60px de large pour "XXX.XX"
            embedded_graphics::primitives::Rectangle::new(Point::new(0, 25), Size::new(128, 25))
                .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
                    BinaryColor::Off,
                ))
                .draw(&mut self.display)
                .map_err(|e| format!("Clear rect error: {:?}", e))?;

            let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
            let text = match bpm {
                Some(b) => format!("{:.2}", b),
                None => String::from("***.**"),
            };
            Text::new(&text, Point::new(35, 45), style)
                .draw(&mut self.display)
                .map_err(|e| format!("Draw error: {:?}", e))?;
            self.display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;
            Ok(())
        }

        pub fn update_audio_bar(&mut self, value: f32) -> Result<(), Box<dyn std::error::Error>> {
            // Valeur entre 0.0 et 0.6
            let clamped = if value < 0.0 {
                0.0
            } else if value > 0.6 {
                0.6
            } else {
                value
            };
            let bar_width = (clamped * 125.0 / 0.6).round() as u32; // Largeur max 125px

            // On efface la zone de la barre audio
            embedded_graphics::primitives::Rectangle::new(Point::new(2, 55), Size::new(125, 8))
                .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
                    BinaryColor::Off,
                ))
                .draw(&mut self.display)
                .map_err(|e| format!("Clear audio bar error: {:?}", e))?;

            // On dessine la nouvelle barre audio
            embedded_graphics::primitives::Rectangle::new(
                Point::new(2, 55),
                Size::new(bar_width, 8),
            )
            .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
                BinaryColor::On,
            ))
            .draw(&mut self.display)
            .map_err(|e| format!("Draw audio bar error: {:?}", e))?;

            self.display
                .flush()
                .map_err(|e| format!("Flush error: {:?}", e))?;
            Ok(())
        }
    }
}
