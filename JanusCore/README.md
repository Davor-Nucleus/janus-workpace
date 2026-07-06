# JanusCore Player

Un lecteur JanusCore en Rust pour la lecture audio.

## 🎵 Fonctionnalités

- **Lecture audio** : Support MP3, WAV, FLAC
- **API REST** : Interface HTTP complète pour contrôler le lecteur
- **Gestion des playlists** : Navigation avant/arrière, mélange aléatoire
- **Limiteur audio** : Limitation du volume maximum en dB pour éviter la saturation
- **Fenêtre GUI de logs (Windows)** : Affichage en temps réel des logs dans une petite fenêtre optionnelle
  - Activation via `env.json` → `janusCoreGui: true` (par défaut)
  - Fermeture de la fenêtre = arrêt propre de l’application (serveur + tâches)
  - Icône personnalisée chargée depuis `favicon.ico`

## 🚀 Installation

```bash
# Cloner le projet
git clone <repository-url>
cd JanusCore

# Installer les dépendances
cargo build

# Lancer le serveur
cargo run
```

## 📁 Structure des dossiers

```
JanusCore/
├── public/music/          # Dossiers de musique
│   ├── rock/             # Exemple de dossier
│   ├── jazz/             # Exemple de dossier
│   └── classical/        # Exemple de dossier
├── src/                  # Code source
├── Cargo.toml           # Dépendances
└── env.json             # Configuration
```

## 🔧 Configuration

Créez un fichier `env.json` à la racine :

```json
{
  "PORT_MUSIC": 3000,
  "VOLUME": 0.8,
  "LIMITER_DB": 0.0,
  "janusCoreGui": true
}
```

### Clé `janusCoreGui`
- `true` (défaut) : ouvre une fenêtre GUI "JanusCore - Logs" et y affiche tous les logs (les logs restent aussi visibles dans la console)
- `false` : pas de fenêtre GUI, logs uniquement en console (mode 100% terminal)

## 🌐 API Endpoints

### Contrôle de lecture

- `POST /api/folder?folder=rock` - Lire un dossier
- `POST /api/stop` - Arrêter la lecture
- `POST /api/pause` - Pause/Reprendre
- `POST /api/next` - Piste suivante
- `POST /api/previous` - Piste précédente

### Volume

- `GET /api/volume` - Obtenir le volume actuel
- `POST /api/volume` - Définir le volume (JSON: `{"volume": 0.8}`)
- `GET /api/volume/add` - Augmenter le volume (+0.05)
- `GET /api/volume/subtract` - Diminuer le volume (-0.05)

### Limiteur

Le limiteur empêche le volume de dépasser une valeur maximale en dB. Si le volume en dB dépasse la limite, il est automatiquement réduit.

- `GET /api/limiter` - Obtenir la limite actuelle en dB
- `POST /api/limiter` - Définir la limite (JSON: `{"limiter_db": -6.0}`)
- `GET /api/limiter/add` - Augmenter la limite (+1.0 dB)
- `GET /api/limiter/subtract` - Diminuer la limite (-1.0 dB)

**Note** : Une valeur de `0.0` dB signifie aucune limitation (volume maximum). Des valeurs négatives (ex: `-6.0` dB) limitent le volume à environ 50% du maximum.

### Informations

- `GET /api/folderlist` - Liste des dossiers disponibles
- `GET /api/current-music` - Musique en cours
- `GET /api/has-next` - Vérifier s'il y a une piste suivante
- `GET /api/has-previous` - Vérifier s'il y a une piste précédente

## 🛠️ Développement

### Structure du code

- `src/model.rs` - Logique métier
- `src/controller.rs` - Gestionnaires HTTP
- `src/routes.rs` - Définition des routes API
- `src/service.rs` - Services métier
- `src/main.rs` - Point d'entrée et configuration
- `src/ui.rs` - Fenêtre Win32 minimaliste pour afficher les logs
- `src/logger.rs` - Logger centralisé (console + GUI si activée)

### Ajouter un nouveau format audio

1. Ajouter l'extension dans `add_folder()` (src/model.rs)
2. Vérifier la compatibilité avec rodio
3. Tester avec des fichiers du nouveau format

## 🧪 Tests

```bash
# Vérifier la compilation
cargo check

# Lancer les tests
cargo test

# Build de production
cargo build --release
```

## 📝 Logs

Le système affiche des informations utiles :

```
Lecture : song.mp3
```

Avec `janusCoreGui: true`, ces logs apparaissent aussi dans la fenêtre "JanusCore - Logs".

## 🪟 Fenêtre GUI et arrêt propre

- Une petite fenêtre Win32 affiche les logs et est rafraîchie périodiquement.
- Quand vous fermez la fenêtre, l'application déclenche un arrêt gracieux du serveur HTTP et des tâches en arrière-plan, puis se termine.
- Avec `janusCoreGui: false`, cette fenêtre n’est pas lancée.

## 🖼️ Icône Windows

- L’icône de l’application est embarquée via `build.rs` et `winres`, en pointant sur `favicon.ico` à la racine.
- Pour être certain que l’icône apparaisse dans la barre des tâches et la barre de titre, la fenêtre charge l’icône intégrée (resource id 1) et la définit explicitement (`WM_SETICON`).
- Construire en release:

```bash
cargo build --release
```

Si l’icône ne s’affiche pas :
- Vérifiez que `favicon.ico` contient au moins 16×16 et 32×32.
- `cargo clean && cargo build --release`

## 🎥 Capture audio (OBS sous Windows 11)

- Essayez "Capture audio d’application (bêta)".
- Sinon, utilisez "Capture audio du bureau (WASAPI)" en sélectionnant le périphérique de sortie utilisé par l’app.
- Option: routez la sortie vers un périphérique virtuel (VB-Audio Cable) et capturez ce périphérique dans OBS.

## 🔮 Améliorations futures

- [ ] Interface web pour contrôler le lecteur
- [ ] Analyse audio complète avec Symphonia
- [ ] Métadonnées audio (ID3, etc.)
- [ ] Streaming en temps réel
- [ ] Support des playlists personnalisées

## 📄 Licence

Ce projet est sous licence MIT.

## 🤝 Contribution

Les contributions sont les bienvenues ! N'hésitez pas à :

1. Signaler des bugs
2. Proposer des améliorations
3. Soumettre des pull requests
4. Améliorer la documentation


