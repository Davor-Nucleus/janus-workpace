<div align="center">
  <!-- TODO: Insérer le logo du projet ici si vous en avez un -->
  <!-- <img src="public/logo.png" alt="Janus Core Logo" width="200"/> -->

  # Janus Core Workspace

  **L'infrastructure audio pour le système de contrôle de stream, avec gestion de la musique et du soundboard.**

  [![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](#)
</div>

---

Ce workspace Cargo contient l'infrastructure audio pour le système de contrôle de stream. Il est composé de trois membres principaux :

1. 🎵 **JanusCore** : Le serveur de lecture de musique (MP3/FLAC/WAV/AAC/MP4) avec normalisation EBU R128.
2. 🔊 **PhonosCore** : Le serveur de soundboard (effets sonores, sans normalisation automatique). 
3. ⚙️ **janus_nucleus** : Une bibliothèque partagée contenant la logique de configuration, de journalisation (logs) et d'interface graphique (GUI).

## 📋 Sommaire

- [Prérequis](#-prérequis)
- [Architecture](#-architecture)
- [Configuration](#-configuration)
- [Démarrage](#-démarrage)
- [Fonctionnalités](#-fonctionnalités)
- [Développement](#-développement)

---

## ⚡ Prérequis

> [!IMPORTANT]
> - **Rust** (installé via rustup)
> - **Windows** (pour l'interface graphique winapi, bien que le code de base soit portable)

---

## 🏗️ Architecture

<details>
<summary><b>Cliquez pour dérouler l'arborescence du projet</b></summary>

```text
janus core/
├── JanusCore/           # Serveur Musique
│   ├── src/             # Code source
│   ├── public/music/    # Dossiers de musique
│   └── env.json         # Configuration JanusCore
├── PhonosCore/          # Serveur Soundboard
│   ├── src/             # Code source
│   ├── public/soundboard/ # Fichiers audio
│   └── env.json         # Configuration PhonosCore
├── janus_nucleus/       # Lib partagée (Config, Logger, GUI)
│   └── src/             # Code source commun
├── Cargo.toml           # Configuration workspace
└── Cargo.lock           # Verrouillage des dépendances
```
</details>

---

## ⚙️ Configuration

Chaque projet possède son propre fichier `env.json` dans son répertoire respectif.

<details>
<summary><b>JanusCore (<code>JanusCore/env.json</code>)</b></summary>

```json
{
  "PORT_MUSIC": 3001,
  "VOLUME": 1.0,
  "janusCoreGui": true,
  "normalizationEnabled": true
}
```
</details>

<details>
<summary><b>PhonosCore (<code>PhonosCore/env.json</code>)</b></summary>

```json
{
  "PORT_SOUNDBOARD": 3002,
  "VOLUME": 1.0,
  "phonosCoreGui": true
}
```
</details>

### Paramètres de Configuration

- `PORT_MUSIC` : Port API pour JanusCore (défaut: 3030 si non spécifié).
- `PORT_SOUNDBOARD` : Port API pour PhonosCore (défaut: 3003 si non spécifié).
- `janusCoreGui` : Active/Désactive la fenêtre de logs native pour JanusCore (défaut: true).
- `phonosCoreGui` : Active/Désactive la fenêtre de logs native pour PhonosCore (défaut: true).
- `VOLUME` : Volume global initial (0.0 à 1.0).
- `normalizationEnabled` : Active la normalisation EBU R128 au démarrage pour JanusCore (défaut: true).

---

## 🚀 Démarrage

### Lancer tout le workspace (vérification uniquement)

```bash
cargo check --workspace
```

### Compiler tout le workspace

```bash
cargo build --workspace
```

### Lancer JanusCore (Musique)

```bash
cd JanusCore
cargo run
```
> [!NOTE]
> **API** : `http://127.0.0.1:3001` (ou port configuré dans `env.json`)

### Lancer PhonosCore (Soundboard)

```bash
cd PhonosCore
cargo run
```
> [!NOTE]
> **API** : `http://127.0.0.1:3002` (ou port configuré dans `env.json`)

---

## 🌟 Fonctionnalités

### 🎵 JanusCore (Musique)

#### Lecture et Navigation
- **Lecture de dossier** : `GET /api/folder?folder=NomDossier` (Lance la lecture depuis `public/music/`)
- **Liste des dossiers** : `GET /api/folderlist` (Retourne tous les dossiers de `public/music/`)

#### Contrôle de Lecture
- **Pause** : `GET /api/pause`
- **Reprendre** : `GET /api/resume`
- **Arrêter** : `GET /api/stop`
- **Piste suivante** : `GET /api/next`
- **Piste précédente** : `GET /api/previous`
- **Vérifier piste suivante** : `GET /api/has_next` (Retourne `true`/`false`)
- **Vérifier piste précédente** : `GET /api/has_previous` (Retourne `true`/`false`)

#### Volume
- **Obtenir le volume** : `GET /api/volume` (0.0 à 1.0)
- **Définir le volume** : `POST /api/volume` (Corps: `{"volume": 0.5}`)
- **Augmenter le volume** : `GET /api/volume/add`
- **Diminuer le volume** : `GET /api/volume/subtract`

#### Normalisation EBU R128
- **État** : `GET /api/normalization` (Retourne `{ "normalization_enabled": true/false }`)
- **Activer / désactiver** : `POST /api/normalization` (Corps : `{ "enabled": false }`)
- **Basculer** : `GET /api/normalization/toggle` (Inverse l'état courant, persiste dans `env.json`)

#### État et Informations
- **État actuel** : `GET /api/status` (Retourne pause, volume, titre en cours, etc.)
- **Musique actuelle** : `GET /api/current_music`
- **WebSocket musique** : `WS /api/current_music_ws` (Mises à jour en temps réel)

### 🔊 PhonosCore (Soundboard)

- **Jouer un son** : `GET /api/soundboard/play?sound=nom_fichier`
  - Joue depuis `public/soundboard/`.
  - **Met automatiquement la musique de JanusCore en pause** et la reprend à la fin.
- **Lister les sons** : `GET /api/soundboard/sounds`
- **Arrêter le son** : `GET /api/soundboard/stop` (Arrête le son et reprend la musique)

### 🎧 Audio

#### Formats Supportés
**JanusCore** (avec normalisation) et **PhonosCore** (sans normalisation) supportent les formats suivants :
- MP3
- FLAC
- WAV
- AAC
- MP4 (ISOM4)

---

## 🛠️ Développement

### Logs
Les logs sont affichés dans :
- Une fenêtre dédiée native Windows (si GUI activée dans `env.json`)
- La console standard

### Compilation

```bash
# Compiler tout le workspace
cargo build --workspace

# Compiler en mode release
cargo build --workspace --release

# Vérifier le code sans compiler
cargo check --workspace
```

### Architecture Interne
- **Warp** : Framework HTTP asynchrone pour les API REST
- **Rodio** : Bibliothèque audio pour la lecture de fichiers
- **Symphonia** : Décodage audio multi-format
- **EBUR128** : Normalisation audio selon la norme EBU R128
- **Tokio** : Runtime asynchrone pour Rust
- **Serde/Serde JSON** : Sérialisation/désérialisation JSON

### CORS
Les deux serveurs ont le support CORS activé pour permettre les requêtes depuis des applications web frontend.

---

<div align="center">
  <i>Développé avec ❤️ en Rust</i>
</div>
