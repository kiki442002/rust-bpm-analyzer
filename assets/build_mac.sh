#!/bin/bash
set -e

TARGET=$1

# If the script is run from the assets folder, go up to the root
if [[ $(basename "$PWD") == "assets" ]]; then
    cd ..
fi

echo "Building project and generating bundle..."

if [ -n "$TARGET" ]; then
    echo "Target specified: $TARGET"
    cargo bundle --release --target "$TARGET"
    BUNDLE_DIR="target/$TARGET/release/bundle/osx"
else
    echo "Native build (no target specified)"
    cargo bundle --release
    BUNDLE_DIR="target/release/bundle/osx"
fi

# Path to the Info.plist file generated in the .app
PLIST_PATH="$BUNDLE_DIR/BPM Analyzer.app/Contents/Info.plist"

if [ ! -f "$PLIST_PATH" ]; then
    echo "Error: Info.plist file not found at: $PLIST_PATH"
    exit 1
fi

echo "Adding microphone permission to $PLIST_PATH..."

# Use plutil to insert the key. If it already exists (unlikely after a clean build), replace it.
if plutil -extract NSMicrophoneUsageDescription xml1 -o - "$PLIST_PATH" > /dev/null 2>&1; then
    plutil -replace NSMicrophoneUsageDescription -string "This application needs access to the microphone to analyze music BPM." "$PLIST_PATH"
else
    plutil -insert NSMicrophoneUsageDescription -string "This application needs access to the microphone to analyze music BPM." "$PLIST_PATH"
fi

echo "Re-signing application (ad-hoc) to avoid 'damaged' error..."
codesign --force --deep --sign - "$BUNDLE_DIR/BPM Analyzer.app"

echo "✅ Done! The application is ready."
echo "You can find it here: $BUNDLE_DIR/BPM Analyzer.app"
