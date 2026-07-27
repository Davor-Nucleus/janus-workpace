# PhonosCore 

## Présentation

Le PhonosCore est un player headless qui lit des effets sonores stockés localement en Rust.


## 🎵 Fonctionnalités

- **Lecture de sons du dossier**: joue un fichier audio par son nom, avec ou sans extension (`mp3`, `wav`, `flac`…)
- **Liste des sons disponibles**: expose un endpoint pour récupérer la liste des fichiers audio disponibles
- **Arrêt global**: stoppe tous les sons en cours de la soundboard
- **Pause automatique de la musique**: met JanusCore en pause avant de jouer un effet, puis reprend la lecture
- **Sons simultanés**: plusieurs sinks en parallèle, chacun avec son propre canal d’arrêt
- **Volume persistant**: garde le volume courant dans `env.json` et l’applique aux nouveaux sons
- **Compatibilité Windows**: définit le titre de la console via WinAPI (optionnel sur d’autres OS)
- **CORS ouvert**: accepte les requêtes cross-origin (GET/POST/etc.)
- **GUI Windows optionnelle (logs en temps réel)**: petite fenêtre "phonosCore - Logs" affichant tous les logs en miroir de la console, avec arrêt propre de l’application à la fermeture (icône intégrée).

## 📋 Sommaire

- [Fonctionnalités](#-fonctionnalités)
- [Installation](#-installation)
- [Structure des dossiers](#-structure-des-dossiers)
- [Configuration](#-configuration)
- [API Endpoints](#-api-endpoints)
- [Développement](#-développement)
- [Tests](#-tests)
- [Améliorations futures](#-améliorations-futures)
- [Licence](#-licence)

## 🚀 Installation

- Prérequis: Rust (toolchain stable), cargo
- Windows recommandé (titre console, périphérique audio par défaut géré par Rodio)

Étapes:
1. Cloner le dépôt
2. Placer vos sons dans `public/soundboard` (ex: `my-sound.mp3`)
3. Configurer `env.json` (voir section Configuration)
4. Compiler et lancer:
```bash
cargo run --release
```
Le serveur démarre par défaut sur `http://127.0.0.1:{PORT_SOUNDBOARD}`.

## 📁 Structure des dossiers

- `src/`: code source Rust
  - `main.rs`: bootstrap, lecture config, init audio, serveur Warp
  - `routes.rs`: définition des routes HTTP
  - `controller.rs`: logique des handlers HTTP (play/list/stop)
  - `model.rs`: état du lecteur, gestion du volume et des sinks, config
  - `service.rs`: services systèmes (audio, titre console)
  - `view.rs`: aides de réponses JSON (réutilisables)
- `public/soundboard/`: fichiers audio joués par la soundboard
- `env.json`: configuration de port/volume/périphérique
- `Cargo.toml`: dépendances (warp, tokio, rodio, serde, reqwest, etc.)

## 🔧 Configuration

Fichier `env.json` (exemple):
```json
{
  "PORT_SOUNDBOARD": 3002,
  "VOLUME": 0.45,
  "janusCoreGui": true
}
```
- **PORT_SOUNDBOARD**: port HTTP de ce serveur (défaut 3003 si absent)
- **VOLUME**: volume initial (0.0 – 1.0), persistant (mis à jour lors des changements)
- **janusCoreGui**: (bool) active la fenêtre GUI de logs.
  - Si `true`: ouvre la fenêtre "phonosCore - Logs" et mirror les logs (console + fenêtre).
  - Si `false`: pas de fenêtre, logs uniquement en console.
  - Si le champ est absent: valeur par défaut `true`.

Variable d’environnement optionnelle:
- **PORT_MUSIC**: port de l’autre application musique (défaut `3001`). Le serveur appelle `http://127.0.0.1:{PORT_MUSIC}/api/pause` pour pause/reprise.

### 🎚️ Périphérique audio

La sortie audio utilise le périphérique par défaut du système (via Rodio). Il n’y a plus de sélection de périphérique dans la configuration.

## 🌐 API Endpoints

Base URL: `http://127.0.0.1:{PORT_SOUNDBOARD}`

### Contrôle de lecture

- `GET /api/soundboard/play?sound={name}`
  - Joue `{name}`. Si l’extension est omise, essaie `mp3`, `wav`, `flac`.
  - Met en pause la musique externe avant de jouer.
  - Réponse 200 text/plain sur succès, 400 si introuvable, 500 si erreur IO/decoder.

- `GET /api/soundboard/stop`
  - Coupe tous les sons de la soundboard.
  - Appelle ensuite la pause de l’app musique (toggle pause), utile pour reprendre.
  - Réponse JSON `{ "message": "Soundboard arrêtée avec succès" }`.

- `GET /api/soundboard/sounds`
  - Liste les sons disponibles dans `public/soundboard` (extensions supportées: `mp3`, `wav`, `flac`, `ogg`, `m4a`).
  - Réponse JSON `{ "sounds": ["file1.mp3", ...] }` ou `{ "error": ... }` si dossier absent.

### Exemple d'utilisation

- Lancer un son par nom sans extension:
```bash
curl "http://127.0.0.1:3002/api/soundboard/play?sound=FOR%20THE%20EMPEROR"
```
- Lister les sons:
```bash
curl "http://127.0.0.1:3002/api/soundboard/sounds"
```
- Stopper tous les sons et reprendre la musique:
```bash
curl "http://127.0.0.1:3002/api/soundboard/stop"
```

## 🛠️ Développement

- Toolchain: Rust 1.75+ recommandé
- Dépendances clés: `warp`, `tokio`, `rodio`, `serde`, `reqwest`
- Exécution locale: `cargo run` (lit `env.json` à chaud pour initialiser le volume/port)

### Logs et GUI

- Tous les logs passent par un logger centralisé: `log_info(...)` et `log_error(...)`.
- Éviter d’utiliser `println!/eprintln!` directement: ils ont été remplacés dans le code.
- Lorsque `janusCoreGui = true` (ou absent), une fenêtre Win32 appelée "phonosCore - Logs" s’ouvre et affiche en temps réel le buffer de logs (wrapping + rafraîchissement périodique). Fermer la fenêtre envoie un signal d’arrêt, déclenchant l’arrêt gracieux du serveur HTTP et des tâches.
- L’icône d’application est intégrée via le script de build (`build.rs`) qui embarque `favicon.ico`. La fenêtre charge l’icône (resource id 1) et l’applique à la barre de titre et la barre des tâches.

### Structure du code

- `main.rs`:
  - lit `env.json` via `read_env_config()` pour `(volume, port)`
  - initialise l’audio `PlayerService::initialize_audio()` et l’état `PlayerState`
  - construit les routes `create_routes(player, music_port)` et applique CORS
- `routes.rs`: déclare `GET /play`, `GET /stop`, `GET /sounds`
- `controller.rs`:
  - `handle_soundboard_play`: résolution de fichier, création `Sink`, application du volume courant, thread de lecture, canal d’arrêt, re-lancement musique si fin naturelle
  - `handle_soundboard_stop`: broadcast arrêt via canaux, vidage des sinks, puis appel pause musique
  - `handle_soundboard_sounds`: lecture du dossier et filtrage extensions
- `model.rs`:
  - `PlayerState`: stocke `stream_handle`, `volume`, `soundboard_sinks`, `soundboard_stop_channels`
  - gestion du volume persistant via `update_env_key("VOLUME", ...)`
- `view.rs`: helpers de réponses JSON (non obligatoires mais prêts à l’emploi)

## 🧪 Tests

- Tests manuels via curl/Postman sur les endpoints.
- Ajouter des tests d’intégration (suggestion) pour:
  - la résolution de fichiers sans extension
  - l’arrêt simultané de multiples sinks
  - la persistance du volume (`env.json` mis à jour)

## 🔮 Améliorations futures

- Endpoint pour régler le volume et le muting
- Logs structurés (tracing) et niveaux de log
- Gestion d’erreurs plus riche (retours JSON homogènes via `view.rs`)
- Packaging multiplateforme (Windows service, systemd, Docker)
- Auth/CORS restrictif selon usage

## 📄 Licence

MIT (à préciser selon votre choix).
