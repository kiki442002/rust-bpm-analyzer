# Rust BPM Analyzer

<p align="center">
  <img src="assets/icon_name.png" alt="Rust BPM Analyzer Logo" width="300"/>
</p>

A real-time BPM analyzer and Ableton Link synchronization tool written in Rust.

![License](https://img.shields.io/badge/license-Non--Commercial-red.svg)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)

## Overview

Rust BPM Analyzer listens to audio input (microphone or loopback), detects the tempo (BPM) in real-time, and synchronizes it with other music software using **Ableton Link**.

It is designed to be lightweight, fast, and cross-platform, running on desktop (macOS, Windows, Linux) and embedded Linux devices (Raspberry Pi, Milk-V Duo).

## Features

*   **Real-time BPM Detection**: Uses energy-based algorithms to detect tempo from audio input.
*   **Ableton Link Support**: Automatically syncs the detected tempo and beat phase with Ableton Live, Traktor, Serato, and other Link-enabled apps.
*   **"Drop" Detection**: Detects sudden energy drops and re-syncs the downbeat automatically.

---

## 🖥️ Desktop (GUI)

Designed for macOS, Windows, and Linux (x86_64). It features a modern interface built with `iced`.

### Installation

#### Download Binaries
Check the [Releases](https://github.com/YOUR_USERNAME/rust-bpm-analyzer/releases) page for the latest pre-compiled binaries:
*   **macOS**: `.dmg` (Universal/Apple Silicon)
*   **Windows**: `.exe` installer
*   **Linux**: `.deb` or `.AppImage`

#### Build from Source
Ensure you have [Rust installed](https://rustup.rs/).

**Prerequisites:**
*   **Linux**: `sudo apt install libasound2-dev`
*   **macOS/Windows**: No extra dependencies required.

```bash
git clone https://github.com/YOUR_USERNAME/rust-bpm-analyzer.git
cd rust-bpm-analyzer
cargo run --release
```

### Usage
1.  **Launch the application**.
2.  **Select Audio Input**: Use the dropdown menu to select the microphone or audio interface listening to the music.
3.  **Enable Detection**: Click "Enable Detection".
4.  **Link**: The app will automatically join the Link session. You should see the "Link Peers" count increase if other Link apps are running on the same network.

---

## 🔌 Embedded (Headless)

Designed for embedded Linux devices like Raspberry Pi, Milk-V Duo, or any headless Linux server. It runs without a monitor and automatically starts analysis on the default audio device.

### Features
*   **Lightweight**: No GUI dependencies, minimal resource usage.
*   **Auto-Start**: Can be configured to run as a systemd service.
*   **Optimized**: Specific optimizations for ARM architectures.

### Cross-Compilation
The project uses `cross` for compiling to ARM architectures.

**Example for Raspberry Pi (64-bit):**
```bash
cross build --release --target aarch64-unknown-linux-gnu
```

**Example for Raspberry Pi (32-bit / ARMv7):**
```bash
cross build --release --target armv7-unknown-linux-gnueabihf
```

### Usage
Transfer the binary to your device and run it:
```bash
./rust-bpm-analyzer
```
*Note: In headless mode, the application will use the system's default audio input device.*

---

## Development

### Project Structure

*   `src/core_bpm/`: Audio capture and BPM analysis logic.
*   `src/network_sync/`: Ableton Link integration.
*   `src/gui.rs`: User interface implementation (Desktop only).
*   `src/embeded/`: Headless implementation (Linux only).
*   `assets/`: Icons and build scripts.

## License

This project is licensed under a **Non-Commercial License**.

You are free to:
*   **Use** the software for personal or non-profit purposes.
*   **Modify** the source code for your own non-commercial use.
*   **Redistribute** the software for free.

You may **NOT**:
*   Use this software for commercial purposes (business, paid services, etc.).
*   Sell this software or any derivative works.

See the [LICENSE](LICENSE) file for the full legal text.
