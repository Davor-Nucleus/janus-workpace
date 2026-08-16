<div align="center">
  <!-- TODO: Insérer le logo du projet ici si vous en avez un -->
  <!-- <img src="public/logo.png" alt="Janus Core Logo" width="200"/> -->

  # Janus Core Workspace

  **L'infrastructure audio pour le système de contrôle de stream, avec gestion de la musique, du soundboard et de la diffusion HTTP.**

  [![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](#)
</div>

---

Ce workspace Cargo contient l'infrastructure audio pour le système de contrôle de stream. Il est composé de quatre membres :

1. 🎵 **JanusCore** : Le serveur de lecture de musique (MP3/FLAC/WAV/AAC/MP4) avec normalisation EBU R128.
2. 🔊 **PhonosCore** : Le serveur de soundboard (effets sonores, sans normalisation automatique). 
3. 📻 **WebRadioCore** : La webradio — diffuse la musique en MP3 sur HTTP, pour l'écouter depuis un autre appareil du réseau.
4. ⚙️ **janus_nucleus** : Une bibliothèque partagée contenant la logique de configuration, de journalisation (logs), d'interface graphique (GUI), de diffusion audio et de lecture des métadonnées.

> [!NOTE]
> JanusCore et PhonosCore jouent sur **la carte son du PC**. WebRadioCore, lui, n'ouvre aucun périphérique
> audio : il décode à la cadence de l'horloge et encode en MP3. C'est un lecteur **indépendant**, avec sa
> propre playlist — il ne rediffuse pas ce que joue JanusCore, les deux tournent en parallèle.

## ✨ Fonctionnalités principales

- **Trois serveurs audio headless** — musique (JanusCore), soundboard (PhonosCore) et webradio (WebRadioCore), pilotables indépendamment.
- **Diffusion HTTP en MP3** — `GET /stream.mp3` : écoute depuis un navigateur, VLC, OBS ou un téléphone du réseau local.
- **Multi-format** — MP3, FLAC, WAV, AAC et MP4 décodés via Symphonia.
- **Playlists par dossier** — un dossier de `public/music/` = une playlist, avec lecture aléatoire.
- **Navigation complète** — piste suivante / précédente avec historique, pause, reprise, arrêt.
- **Normalisation EBU R128** — cible −14 LUFS avec cache par fichier, activable/désactivable à chaud.
- **Ducking automatique** — PhonosCore met la musique de JanusCore en pause pendant un effet sonore, puis la reprend.
- **Métadonnées enrichies** — tags ID3/Vorbis (titre, artiste, album, date) et pochette en base64.
- **API REST complète** — lecture, volume, état et normalisation exposés en HTTP.
- **WebSocket temps réel** — flux de l'état du lecteur pour les overlays OBS.
- **Volume persistant** — sauvegardé dans `env.json` et réappliqué au démarrage.
- **Fenêtre de logs Windows** — GUI Win32 optionnelle affichant les logs en temps réel.
- **CORS ouvert** — appels directs depuis n'importe quel frontend web.

## 📋 Sommaire

- [Fonctionnalités principales](#-fonctionnalités-principales)
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
├── WebRadioCore/        # Serveur Webradio (diffusion HTTP)
│   ├── src/             # Code source
│   ├── public/music/    # Dossiers de musique
│   └── env.json         # Configuration WebRadioCore
├── janus_nucleus/       # Lib partagée (Config, Logger, GUI, Stream)
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

<details>
<summary><b>WebRadioCore (<code>WebRadioCore/env.json</code>)</b></summary>

```json
{
  "PORT_WEBRADIO": 3005,
  "WEBRADIO_VOLUME": 1.0,
  "webRadioBind": "0.0.0.0",
  "webRadioBitrate": 192,
  "webRadioCoreGui": true,
  "webRadioNormalization": true
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
- `PORT_WEBRADIO` : Port API et flux pour WebRadioCore (défaut: 3005).
- `WEBRADIO_VOLUME` : Volume de la webradio (0.0 à 1.0, défaut: 1.0). Clé **distincte** de `VOLUME`, qui est déjà partagée entre JanusCore et PhonosCore — sans quoi régler le volume de la radio changerait celui de la lecture locale.
- `webRadioBind` : Interface d'écoute de WebRadioCore (défaut: `0.0.0.0`, soit tout le réseau local). Une valeur illisible fait replier sur `127.0.0.1`, jamais sur `0.0.0.0`.
- `webRadioBitrate` : Débit du flux MP3 en kbps (défaut: 192). Ramené automatiquement à la valeur LAME la plus proche.
- `webRadioCoreGui` : Active/Désactive la fenêtre de logs native pour WebRadioCore (défaut: true).
- `webRadioNormalization` : Active la normalisation EBU R128 pour la webradio (défaut: true). Clé distincte de `normalizationEnabled`, pour la même raison que le volume.

> [!WARNING]
> Avec `webRadioBind` à `0.0.0.0`, **l'API de contrôle est exposée au réseau local**, pas seulement le flux.
> Le CORS ne protège que les navigateurs : n'importe qui sur le réseau peut appeler `/api/folder` avec
> `curl`. Acceptable sur un réseau domestique de confiance ; passer à `127.0.0.1` sinon.

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

### Lancer WebRadioCore (Webradio)

```bash
cd WebRadioCore
cargo run
```
> [!NOTE]
> **Flux** : `http://<ip-locale>:3005/stream.mp3` — à ouvrir dans VLC, un navigateur ou sur un téléphone
> connecté au même réseau. La radio diffuse du silence tant qu'aucune playlist n'est chargée : on peut donc
> s'y connecter avant de lancer la musique.

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

### 📻 WebRadioCore (Webradio)

#### Flux et playlists
- **Flux MP3** : `GET /stream.mp3` (48 kHz, stéréo, débit configurable)
- **Charger une playlist** : `GET /api/folder?folder=NomDossier` (depuis `public/music/`)
- **Lister les dossiers** : `GET /api/folderlist`

#### Contrôle de lecture
- **Piste suivante** : `GET /api/next`
- **Piste précédente** : `GET /api/previous`
- **Vérifier piste suivante** : `GET /api/has_next` (toujours `true` dès qu'un dossier est chargé — la radio reboucle)
- **Vérifier piste précédente** : `GET /api/has_previous`
- **Pause** : `GET /api/pause`
- **Reprendre** : `GET /api/resume`

> [!NOTE]
> **La pause gèle la piste, elle n'arrête pas le flux.** Les auditeurs restent connectés et entendent du
> silence ; `resume` repart exactement à l'échantillon où la piste s'était arrêtée, et non plus loin dans le
> morceau. Il n'y a volontairement pas de `/api/stop` : charger une autre playlist suffit.

#### Volume
- **Obtenir le volume** : `GET /api/volume`
- **Définir le volume** : `POST /api/volume` (corps : `{"volume": 0.5}`, borné à `[0,1]`, persisté dans `env.json`)

Le moteur relit le volume à chaque bloc, donc un changement en cours de piste s'entend immédiatement **sans
perdre le gain de normalisation** — contrairement à `JanusCore::set_volume`, qui écrase ce gain jusqu'au
morceau suivant.

#### État et informations
- **État** : `GET /api/status` → `{playing, paused, current_music, volume, queue_len, has_next, has_previous, listeners, bitrate_kbps, stream_url}`
- **Musique actuelle** : `GET /api/current_music` (titre, artiste, album, date, pochette en base64)
- **WebSocket** : `WS /api/current_music_ws` — instantané poussé **uniquement quand il change**, sondé toutes les 500 ms

#### Comportement du flux

- **Silence quand rien ne joue** : le flux ne se ferme jamais. Un auditeur connecté avant le chargement
  d'une playlist entend la musique démarrer sans avoir à se reconnecter.
- **Enchaînement sans blanc** : les pistes se succèdent à l'intérieur d'un même bloc encodé.
- **Rebouclage infini** : en fin de playlist, le dossier est relu et remélangé.
- **Clients lents tolérés** : un auditeur en retard saute les morceaux perdus et se resynchronise, plutôt
  que d'être déconnecté — les trames MP3 étant auto-délimitées.
- **Normalisation non bloquante** : l'analyse EBU R128 d'une piste inconnue tourne en tâche de fond et
  précharge la piste suivante. Une piste jamais analysée passe une fois sans gain plutôt que de figer le
  flux pour tous les auditeurs.

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
- **mp3lame-encoder** : Encodage MP3 du flux WebRadioCore (LAME compilé depuis ses sources, aucune DLL à déployer — licence LGPL-3.0)
- **Tokio** : Runtime asynchrone pour Rust
- **Serde/Serde JSON** : Sérialisation/désérialisation JSON

### Les features de janus_nucleus

Trois modules optionnels, activés à la demande par les binaires qui en ont besoin :

| Feature | Contenu | Utilisée par |
|---|---|---|
| `stream` | Cadenceur horloge, encodeur MP3 (LAME), diffusion un-vers-N | WebRadioCore |
| `metadata` | Tags ID3/Vorbis, pochette base64, validation du MIME de pochette | JanusCore, WebRadioCore |
| `music` | Découverte des playlists : listage des dossiers, collecte récursive et mélange des pistes | JanusCore, WebRadioCore |

Les modules **toujours disponibles** sont `config` (dont la politique d'origines CORS), `logger`, `paths`
(garde anti-traversée), `audio` (normalisation EBU R128), `console` (titre de la fenêtre) et, sous Windows,
`gui`.

`stream` est isolée derrière une feature pour que JanusCore et PhonosCore n'aient pas à compiler LAME
depuis ses sources C :

```bash
cd JanusCore && cargo build          # sans LAME
cd WebRadioCore && cargo build       # avec LAME
cargo build --workspace              # LAME compilé une fois (les features sont unifiées)
```

`metadata` est partagée plutôt que dupliquée parce qu'elle contient `sanitize_cover_mime`, un **contrôle de
sécurité** : il empêche un `media_type` forgé dans un tag de s'échapper de l'attribut `src` d'un overlay.
Une seconde copie de ce contrôle finirait tôt ou tard par diverger de la première.

### CORS
Les deux serveurs ont le support CORS activé pour permettre les requêtes depuis des applications web frontend.

---

<div align="center">
  <i>Développé avec ❤️ en Rust</i>
</div>
