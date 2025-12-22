mod core_bpm;
mod network_sync;

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
mod embeded;
#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod gui;

// Configuration grouped by platform
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
mod platform {
    pub const SAMPLE_RATE: u32 = 11025;
    pub const HOP_SIZE: usize = SAMPLE_RATE as usize;

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting embedded Mode...");
        super::embeded::run()
    }
}

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod platform {
    pub const SAMPLE_RATE: u32 = 44100;
    pub const HOP_SIZE: usize = SAMPLE_RATE as usize;

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting GUI Mode...");
        super::gui::run()
    }
}

pub use platform::SAMPLE_RATE;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::run()
}
