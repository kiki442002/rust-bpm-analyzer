#!/bin/bash
set -e

TARGET=$1

# Si le script est lancé depuis le dossier assets, on remonte à la racine
if [[ $(basename "$PWD") == "assets" ]]; then
    cd ..
fi

echo "Construction du projet et génération du bundle..."

if [ -n "$TARGET" ]; then
    echo "Target spécifiée : $TARGET"
    cargo bundle --release --target "$TARGET"
    BUNDLE_DIR="target/$TARGET/release/bundle/osx"
else
    echo "Build natif (pas de target spécifiée)"
    cargo bundle --release
    BUNDLE_DIR="target/release/bundle/osx"
fi

# Chemin vers le fichier Info.plist généré dans le .app
PLIST_PATH="$BUNDLE_DIR/BPM Analyzer.app/Contents/Info.plist"

if [ ! -f "$PLIST_PATH" ]; then
    echo "Erreur : Le fichier Info.plist n'a pas été trouvé à l'emplacement : $PLIST_PATH"
    exit 1
fi

echo "Ajout de la permission microphone dans $PLIST_PATH..."

# On utilise plutil pour insérer la clé. Si elle existe déjà (peu probable après un clean build), on la remplace.
if plutil -extract NSMicrophoneUsageDescription xml1 -o - "$PLIST_PATH" > /dev/null 2>&1; then
    plutil -replace NSMicrophoneUsageDescription -string "Cette application a besoin d'accéder au microphone pour analyser le BPM de la musique." "$PLIST_PATH"
else
    plutil -insert NSMicrophoneUsageDescription -string "Cette application a besoin d'accéder au microphone pour analyser le BPM de la musique." "$PLIST_PATH"
fi

echo "Re-signature de l'application (ad-hoc) pour éviter l'erreur 'endommagé'..."
codesign --force --deep --sign - "$BUNDLE_DIR/BPM Analyzer.app"

echo "✅ Terminé ! L'application est prête."
echo "Vous pouvez la trouver ici : $BUNDLE_DIR/BPM Analyzer.app"
