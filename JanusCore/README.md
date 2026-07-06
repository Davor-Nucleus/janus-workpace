# JanusCore

Serveur audio headless en Rust pour la lecture de musique. Conçu pour être contrôlé via une API REST et intégré à des overlays OBS via WebSocket.

## Fonctionnalités

- **Formats supportés** : MP3, FLAC, WAV, AAC, MP4 (via Symphonia)
- **Playlists par dossier** : chargement et lecture aléatoire d'un dossier entier
- **Navigation** : piste suivante / précédente avec historique
- **Normalisation EBU R128** : analyse automatique de la loudness à −14.0 LUFS avec cache, activable/désactivable à chaud
- **Métadonnées** : lecture des tags ID3/Vorbis (titre, artiste, album, date, pochette en base64)
- **WebSocket** : flux temps réel de l'état du lecteur pour les overlays
- **Fenêtre de logs Windows** : fenêtre Win32 optionnelle affichant les logs en temps réel

## Configuration

Créez `env.json` à la racine du binaire :

```json
{
  "PORT_MUSIC": 3001,
  "VOLUME": 0.8,
  "janusCoreGui": true,
  "normalizationEnabled": true
}
```

| Clé | Type | Description |
|-----|------|-------------|
| `PORT_MUSIC` | int | Port HTTP du serveur |
| `VOLUME` | float (0.0–1.0) | Volume initial |
| `janusCoreGui` | bool | Ouvre la fenêtre de logs Win32 |
| `normalizationEnabled` | bool | Active la normalisation EBU R128 au démarrage (défaut : `true`) |

## Structure des dossiers

```
JanusCore/
├── public/music/      # Dossiers de musique (un dossier = une playlist)
│   ├── rock/
│   └── jazz/
├── src/
├── Cargo.toml
└── env.json
```

## API REST

Toutes les routes sont en **GET** sauf indication contraire.

### Lecture

| Route | Description |
|-------|-------------|
| `GET /api/folderlist` | Liste des dossiers disponibles sous `public/music/` |
| `GET /api/folder?folder=<nom>` | Charge et lance la lecture d'un dossier |
| `GET /api/pause` | Met en pause |
| `GET /api/resume` | Reprend la lecture |
| `GET /api/stop` | Arrête et vide la playlist |
| `GET /api/next` | Piste suivante |
| `GET /api/previous` | Piste précédente |
| `GET /api/has_next` | `{ "has_next": bool, "queue_length": int, "current_music": str }` |
| `GET /api/has_previous` | `{ "has_previous": bool, "previous_music": str }` |

### Volume

| Route | Description |
|-------|-------------|
| `GET /api/volume` | `{ "volume": float }` |
| `POST /api/volume` | Body : `{ "volume": 0.8 }` |
| `GET /api/volume/add` | +0.05 |
| `GET /api/volume/subtract` | −0.05 |

### Normalisation EBU R128

| Route | Description |
|-------|-------------|
| `GET /api/normalization` | `{ "normalization_enabled": bool }` |
| `POST /api/normalization` | Body : `{ "enabled": false }` — active ou désactive |
| `GET /api/normalization/toggle` | Bascule l'état courant |

L'état est persisté dans `env.json` (clé `normalizationEnabled`) et relu au prochain démarrage.

### Informations

| Route | Description |
|-------|-------------|
| `GET /api/status` | État complet : `{ "paused", "current_music", "volume" }` |
| `GET /api/current_music` | Métadonnées de la piste en cours (voir ci-dessous) |
| `WS  /api/current_music_ws` | Flux WebSocket de l'état du lecteur |

#### Réponse de `/api/current_music`

```json
{
  "filename": "04 - Theme.mp3",
  "title": "Theme",
  "artist": "Evan Call",
  "album": "OST",
  "date": "2023",
  "cover_art": "data:image/jpeg;base64,/9j/..."
}
```

Les champs `title`, `artist`, `album`, `date`, `cover_art` sont `null` si absents des tags du fichier.  
Si aucune musique ne joue : `{ "message": "Aucune musique en cours de lecture." }`

#### Payload WebSocket (`/api/current_music_ws`)

Envoyé à chaque changement d'état (polling 500 ms) :

```json
{
  "queue_len": 12,
  "has_sink": true,
  "paused": false,
  "volume": 0.8,
  "current_music": "04 - Theme.mp3",
  "has_next": true,
  "history_len": 3,
  "metadata": {
    "filename": "04 - Theme.mp3",
    "title": "Theme",
    "artist": "Evan Call",
    "album": "OST",
    "date": "2023",
    "cover_art": "data:image/jpeg;base64,..."
  }
}
```

Les métadonnées ne sont relues depuis le fichier qu'au changement de piste (cache par chemin).

## Normalisation EBU R128

Chaque fichier est analysé une fois (gain calculé pour atteindre −14.0 LUFS) puis mis en cache. Le gain est appliqué en combinaison avec le volume utilisateur :

```
volume_final = volume_utilisateur × gain_normalisation
```

Quand la normalisation est **désactivée**, `gain_normalisation = 1.0` (volume brut du fichier). Le cache reste intact — réactiver la normalisation n'implique pas de ré-analyse.

Le toggle est disponible à chaud via `GET /api/normalization/toggle` ou `POST /api/normalization` sans redémarrage.

## Fenêtre GUI (Windows)

Avec `janusCoreGui: true` :
- Une fenêtre Win32 "JanusCore - Logs" s'ouvre au démarrage
- Les logs s'y affichent en temps réel (et restent aussi dans la console)
- Fermer la fenêtre déclenche un arrêt propre du serveur

Avec `janusCoreGui: false` : logs console uniquement.

## Build

```bash
# Depuis le workspace janus core/
cargo build -p JanusCore --release
```

Le binaire est dans `target/release/JanusCore.exe`.

## Structure du code

| Fichier | Rôle |
|---------|------|
| `src/main.rs` | Point d'entrée, démarrage du serveur |
| `src/model.rs` | `PlayerState`, logique de playlist, lecture de métadonnées |
| `src/controller.rs` | Handlers HTTP et WebSocket |
| `src/routes.rs` | Définition des routes Warp |
| `src/service.rs` | Thread d'auto-play |

Les utilitaires partagés (normalisation EBU R128, config, logger, GUI) sont dans le crate `janus_nucleus`.
