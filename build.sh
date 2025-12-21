#!/bin/bash
set -e

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo "Error: 'cross' is not installed."
    echo "Please install it using: cargo install cross"
    exit 1
fi

# Create dist directory
mkdir -p dist

echo "🚀 Starting Build Process..."

# 1. macOS (Native - Apple Silicon)
echo "🍏 Building for macOS (Apple Silicon)..."
cargo build --release --target aarch64-apple-darwin
cp target/aarch64-apple-darwin/release/rust-bpm-analyzer dist/rust-bpm-analyzer-aarch64-apple-darwin

# 2. Linux x86_64 (via Cross)
echo "🐧 Building for Linux (x86_64)..."
CARGO_TARGET_DIR=target/cross-x86_64 cross build --release --target x86_64-unknown-linux-gnu
cp target/cross-x86_64/x86_64-unknown-linux-gnu/release/rust-bpm-analyzer dist/rust-bpm-analyzer-x86_64-linux

# 3. Linux ARM64 (via Cross)
echo "🍓 Building for Linux (ARM64)..."
CARGO_TARGET_DIR=target/cross-aarch64 cross build --release --target aarch64-unknown-linux-gnu
cp target/cross-aarch64/aarch64-unknown-linux-gnu/release/rust-bpm-analyzer dist/rust-bpm-analyzer-aarch64-linux

# 4. Windows (via Cross)
echo "🪟 Building for Windows (x86_64)..."
CARGO_TARGET_DIR=target/cross-windows cross build --release --target x86_64-pc-windows-gnu
cp target/cross-windows/x86_64-pc-windows-gnu/release/rust-bpm-analyzer.exe dist/rust-bpm-analyzer-windows.exe

echo "✅ Build Complete! Artifacts are in dist/"
