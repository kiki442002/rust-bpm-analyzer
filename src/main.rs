#![windows_subsystem = "windows"]

mod core_bpm;
mod core_embedded;
mod network_sync;

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
pub mod midi;

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
mod embedded;
#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod gui;

// Configuration grouped by platform
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
mod platform {
    pub const TARGET_SAMPLE_RATE: u32 = 12000;

    pub async fn run_async() -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting embedded Mode...");
        super::embedded::run().await
    }
}

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
mod platform {
    pub const TARGET_SAMPLE_RATE: u32 = 48000;

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting GUI Mode...");
        super::gui::run()
    }
}

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::run_async().await
}

#[cfg(not(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    platform::run()
}
