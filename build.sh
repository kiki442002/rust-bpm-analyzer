#!/bin/bash
set -e

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo "Error: 'cross' is not installed."
    echo "Please install it using: cargo install cross"
    exit 1
fi

echo "🚀 Starting Build Process..."

# 1. macOS (Native - Apple Silicon)
echo "🍏 Building for macOS (Apple Silicon)..."
cargo build --release --target aarch64-apple-darwin

# 2. Linux x86_64 (via Cross)
echo "🐧 Building for Linux (x86_64)..."
cross build --release --target x86_64-unknown-linux-gnu

# 3. Linux ARM64 (via Cross)
echo "🍓 Building for Linux (ARM64)..."
cross build --release --target aarch64-unknown-linux-gnu

# 4. Windows (via Cross)
echo "🪟 Building for Windows (x86_64)..."
cross build --release --target x86_64-pc-windows-gnu

echo "✅ Build Complete! Artifacts are in target/"
