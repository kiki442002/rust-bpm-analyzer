#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod diplay;
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod led;
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod update;

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub use diplay::display::BpmDisplay;

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub use led::Led;

#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub use update::update::Updater;
