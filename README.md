# Janus Core Workspace

Ce workspace Cargo contient l'infrastructure audio pour le système de contrôle de stream. Il est composé de trois membres principaux :

1.  **JanusCore** : Le serveur de lecture de musique (MP3/FLAC/WAV).
2.  **PhonosCore** : Le serveur de soundboard (effets sonores).
3.  **janus_common** : Une bibliothèque partagée contenant la logique de configuration, de journalisation (logs) et d'interface graphique (GUI).

## Prérequis

*   Rust (installé via rustup)
*   Windows (pour l'interface graphique winapi, bien que le code de base soit portable)

## Structure du Projet

```
janus core/
├── JanusCore/       # Serveur Musique (Port 3030 par défaut)
├── PhonosCore/      # Serveur Soundboard (Port 3003 par défaut)
├── janus_common/    # Lib partagée (Config, Logger, GUI)
├── env.json         # Fichier de configuration unique
└── public/          # Dossiers de médias
    ├── music/       # Dossiers de musique pour JanusCore
    └── soundboard/  # Fichiers audio pour PhonosCore
```

## Configuration (`env.json`)

{
  "PORT_MUSIC": 3001,
  "PORT_SOUNDBOARD": 3002,
  "janusCoreGui": true,
  "phonosCoreGui": true,
  "VOLUME": 1.0,
  "LIMITER_DB": 0.0
}
```

*   **PORT_MUSIC** : Port API pour JanusCore.
*   **PORT_SOUNDBOARD** : Port API pour PhonosCore.
*   **janus_core_gui** / **phonosCoreGui** : Active/Désactive la fenêtre de logs native.
*   **VOLUME** : Volume global initial (0.0 à 1.0).
*   **LIMITER_DB** : Limiteur en décibels.

## Démarrage

Pour lancer les applications, utilisez `cargo run` depuis la racine ou dans chaque dossier dossier.

### Lancer tout le workspace (check seulement)
```bash
cargo check --workspace
```

### Lancer JanusCore (Musique)
```bash
cd JanusCore
cargo run
```
API: `http://127.0.0.1:3001`

### Lancer PhonosCore (Soundboard)
```bash
cd PhonosCore
cargo run
```
API: `http://127.0.0.1:3002`

## fonctionnalités

### JanusCore (Musique)
*   **Lecture de dossier** : `/api/folder?folder=NomDossier`
*   **Contrôle** : `/api/pause`, `/api/resume`, `/api/stop`, `/api/next`, `/api/previous`
*   **Volume** : `/api/volume` (GET/POST)
*   **Status** : `/api/status` (Retourne état pause, volume, titre en cours)
*   **Websocket** : `/api/current_music_ws`

### PhonosCore (Soundboard)
*   **Jouer un son** : `/api/soundboard/play?sound=nom_fichier`
    *   *Note : Met automatiquement la musique de JanusCore en pause et la reprend à la fin du son.*
*   **Lister les sons** : `/api/soundboard/sounds`
*   **Stop** : `/api/soundboard/stop`

## Développement

*   **Logs** : Les logs sont affichés dans une fenêtre dédiée (si GUI activée) et dans la console.
*   **Compilation** : Utilisez `cargo build --workspace` pour compiler tout le projet.
