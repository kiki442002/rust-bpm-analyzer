#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod diplay;
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod led;
#[cfg(all(any(target_arch = "aarch64", target_arch = "arm"), target_os = "linux"))]
pub mod update;
