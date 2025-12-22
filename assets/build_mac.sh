#!/bin/bash
set -e

# Si le script est lancé depuis le dossier assets, on remonte à la racine
if [[ $(basename "$PWD") == "assets" ]]; then
    cd ..
fi

echo "Construction du projet et génération du bundle..."
cargo bundle --release

# Chemin vers le fichier Info.plist généré dans le .app
PLIST_PATH="target/release/bundle/osx/BPM Analyzer.app/Contents/Info.plist"

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

echo "✅ Terminé ! L'application est prête."
echo "Vous pouvez la trouver ici : target/release/bundle/osx/BPM Analyzer.app"
