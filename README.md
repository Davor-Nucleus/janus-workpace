<div align="center">
  <!-- TODO: Insérer le logo du projet ici si vous en avez un -->
  <!-- <img src="public/logo.png" alt="Janus Core Logo" width="200"/> -->

  # Janus Core Workspace

  **L'infrastructure audio pour le système de contrôle de stream, avec gestion de la musique, du soundboard et de la diffusion HTTP.**

  [![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](#)
</div>

---

Ce workspace Cargo contient l'infrastructure audio pour le système de contrôle de stream. Il est composé de cinq membres :

1. 🎵 **JanusCore** : Le serveur de lecture de musique (MP3/FLAC/WAV/AAC/MP4) avec normalisation EBU R128.
2. 🔊 **PhonosCore** : Le serveur de soundboard (effets sonores, sans normalisation automatique). 
3. 📻 **WebRadioCore** : La webradio — diffuse la musique en MP3 sur HTTP, pour l'écouter depuis un autre appareil du réseau.
4. 🎹 **OrpheusCore** : Le générateur — **synthétise** de la synthwave en continu et la diffuse en MP3. Aucun fichier audio.
5. ⚙️ **janus_nucleus** : Une bibliothèque partagée contenant la logique de configuration, de journalisation (logs), d'interface graphique (GUI), de diffusion audio et de lecture des métadonnées.

> [!NOTE]
> JanusCore et PhonosCore jouent sur **la carte son du PC**. WebRadioCore et OrpheusCore n'ouvrent aucun
> périphérique audio : ils produisent leur flux à la cadence de l'horloge et l'encodent en MP3. Les quatre
> serveurs sont indépendants et peuvent tourner en parallèle.
>
> Seul OrpheusCore ne lit **aucun** fichier : sa musique n'existe nulle part avant d'être calculée.

## ✨ Fonctionnalités principales

- **Quatre serveurs audio headless** — musique (JanusCore), soundboard (PhonosCore), webradio (WebRadioCore) et génération (OrpheusCore), pilotables indépendamment.
- **Diffusion HTTP en MP3** — `GET /stream.mp3` : écoute depuis un navigateur, VLC, OBS ou un téléphone du réseau local.
- **Synthwave générée en direct** — oscillateurs anti-repliés, filtres résonants, batterie synthétisée et arrangement aléatoire reproductible par graine.
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
├── OrpheusCore/         # Generateur Synthwave (aucun fichier audio)
│   ├── src/synth/       # Oscillateurs, enveloppes, filtres, batterie, effets
│   ├── src/compose/     # Theorie musicale et arrangement
│   └── env.json         # Configuration OrpheusCore
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

<details>
<summary><b>OrpheusCore (<code>OrpheusCore/env.json</code>)</b></summary>

```json
{
  "PORT_ORPHEUS": 3006,
  "ORPHEUS_VOLUME": 0.8,
  "orpheusBind": "0.0.0.0",
  "orpheusBitrate": 192,
  "orpheusCoreGui": true,
  "orpheusBpm": 92
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

- `PORT_ORPHEUS` : Port API et flux pour OrpheusCore (défaut: 3006).
- `ORPHEUS_VOLUME` : Volume du générateur (0.0 à 1.0, défaut: 0.8). Clé dédiée, comme les précédentes.
- `orpheusBind` : Interface d'écoute (défaut: `0.0.0.0`). Une valeur illisible fait replier sur `127.0.0.1`.
- `orpheusBitrate` : Débit du flux MP3 en kbps (défaut: 192).
- `orpheusCoreGui` : Active/Désactive la fenêtre de logs native (défaut: true).
- `orpheusBpm` : Tempo de consigne (défaut: 92). Le tempo réel est tiré à ±6 BPM autour de cette valeur.
- `orpheusSeed` : Graine du générateur. **Absente**, elle est tirée au démarrage : chaque lancement produit une musique différente. **Fixée**, la même session se rejoue à l'identique.

> [!WARNING]
> Avec `webRadioBind` ou `orpheusBind` à `0.0.0.0`, **l'API de contrôle est exposée au réseau local**, pas
> seulement le flux. Le CORS ne protège que les navigateurs : n'importe qui sur le réseau peut appeler
> `/api/folder` avec `curl`. Acceptable sur un réseau domestique de confiance ; passer à `127.0.0.1` sinon.

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

### Lancer OrpheusCore (Synthwave générée)

```bash
cd OrpheusCore
cargo run
```
> [!NOTE]
> **Flux** : `http://<ip-locale>:3006/stream.mp3`. Rien à préparer : la musique commence dès le démarrage,
> il n'y a ni playlist ni dossier à fournir.

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

### 🎹 OrpheusCore (Synthwave générée)

| Méthode | Route | Rôle |
|---|---|---|
| GET | `/stream.mp3` | Le flux, 48 kHz stéréo |
| GET | `/api/status` | `{bpm, key, chord, bar, section, energy, seed, volume, listeners, …}` |
| GET / POST | `/api/volume` | Corps : `{"volume": 0.5}` |
| GET | `/api/volume/subtract` | Baisse d'un cran (−0,05) |
| GET | `/api/volume/add` | Monte d'un cran (+0,05) |
| GET | `/api/regenerate` | Nouvelle graine : autre tonalité, autre progression, autre tempo — sans couper le flux |

Les deux routes à cran sont en GET et sans corps, pour être appelables depuis un bouton de Stream Deck.
Arrivées à 0 ou à 1 elles ne font plus rien plutôt que d'échouer : une touche maintenue enfoncée ne doit pas
se mettre à renvoyer des erreurs.

#### Comment la musique est fabriquée

Rien n'est échantillonné : chaque son est calculé.

- **Oscillateurs anti-repliés (PolyBLEP)** — une dent de scie naïve replie ses harmoniques au-dessus de
  Nyquist et sonne métallique ; la correction polynomiale au point de discontinuité l'évite.
- **Filtre résonant TPT** — la forme à transformation préservant la topologie, inconditionnellement stable,
  et non la forme de Chamberlin, qui diverge quand la coupure monte à forte résonance.
- **Quatre voix** — nappe (3 scies désaccordées + sous-octave), basse, arpège pincé, lead avec glissando.
- **Batterie synthétisée** — grosse caisse à hauteur descendante, caisse claire à réverbération *gatée*,
  charleys en bruit filtré.
- **Sidechain** — chaque grosse caisse fait plonger nappe, basse et arpège d'environ 9 dB puis les laisse
  remonter. C'est le « pompage » caractéristique du genre.
- **Arrangement** — progressions mineures classiques (`i–VI–III–VII`, `i–VII–VI–VII`, …), niveau d'énergie
  qui évolue toutes les 8 mesures et décide quels instruments jouent et combien le filtre s'ouvre.
- **Reproductible** — tout l'aléa vient d'une graine unique : à graine égale, la musique est identique
  échantillon pour échantillon.

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
- **mp3lame-encoder** : Encodage MP3 des flux WebRadioCore et OrpheusCore (LAME compilé depuis ses sources, aucune DLL à déployer — licence LGPL-3.0)
- **Tokio** : Runtime asynchrone pour Rust
- **Serde/Serde JSON** : Sérialisation/désérialisation JSON

### Les features de janus_nucleus

Quatre modules optionnels, activés à la demande par les binaires qui en ont besoin :

| Feature | Contenu | Utilisée par |
|---|---|---|
| `stream` | Cadenceur horloge, encodeur MP3 (LAME), diffusion un-vers-N, boucle de production | WebRadioCore, OrpheusCore |
| `http` | Réponse HTTP d'un flux continu (implique `stream`, tire `warp`) | WebRadioCore, OrpheusCore |
| `metadata` | Tags ID3/Vorbis, pochette base64, validation du MIME de pochette | JanusCore, WebRadioCore |
| `music` | Découverte des playlists : listage des dossiers, collecte récursive et mélange des pistes | JanusCore, WebRadioCore |

Les modules **toujours disponibles** sont `config` (politique d'origines CORS, analyse d'adresse d'écoute,
persistance de clés), `logger`, `paths` (garde anti-traversée), `audio` (normalisation EBU R128), `console`
(titre de la fenêtre) et, sous Windows, `gui`.

Trois briques ont été mutualisées parce que leur divergence coûterait cher :

- **`stream::http::stream_response`** — les en-têtes d'un flux sans fin. Ajouter un `Content-Length`, même
  « pour bien faire », le casse : c'est son absence qui fait basculer hyper en `Transfer-Encoding: chunked`.
- **`config::parse_bind`** — une adresse illisible se replie sur `127.0.0.1`, **jamais** sur `0.0.0.0`. Une
  coquille dans `env.json` doit fermer le serveur, pas l'ouvrir au réseau.
- **`stream::spawn_encode_loop`** — la boucle produire → encoder → publier → cadencer. Les deux serveurs de
  flux ne diffèrent que par le remplissage d'un bloc de PCM ; tout le reste leur est commun.

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

### Le profil de compilation d'OrpheusCore

La racine du workspace force `opt-level = 2` sur OrpheusCore, y compris en debug :

```toml
[profile.dev.package.OrpheusCore]
opt-level = 2
```

Sa synthèse travaille échantillon par échantillon — environ 96 000 passages de boucle par seconde d'audio.
Non optimisée, elle n'atteint pas le temps réel et le flux hacherait. Les autres crates n'en ont pas besoin :
elles ne font que décoder.

### CORS
Les deux serveurs ont le support CORS activé pour permettre les requêtes depuis des applications web frontend.

---

<div align="center">
  <i>Développé avec ❤️ en Rust</i>
</div>
