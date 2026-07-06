//! Core data model and playback logic for the headless MP3 server.
//! Exposes configuration types (`EnvConfig`, `VolumeRequest`) and the player state (`PlayerState`).
//! Also provides helpers to read/write `env.json` and to discover music folders.

use janus_nucleus::audio::NormalizationManager;
use janus_nucleus::config::update_config_key;
use janus_nucleus::logger::{log_error, log_info};
use rand::seq::SliceRandom;
use rodio::{Decoder, Sink};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;
use std::{
    collections::VecDeque, fmt, fs::File, io::BufReader, path::Path, path::PathBuf, sync::Arc,
};

#[derive(Serialize)]
pub struct MusicMetadata {
    pub filename: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<String>,
    pub cover_art: Option<String>,
}

#[derive(Deserialize)]
/// JSON request body for updating the player volume.
pub struct VolumeRequest {
    pub volume: f32,
}

#[derive(Deserialize)]
/// JSON request body for updating the normalization state.
pub struct NormalizationRequest {
    pub enabled: bool,
}

/// Represents the current state of the player: queue, current track,
/// volume and pause state, plus a bounded history to navigate backwards.

pub struct PlayerState {
    pub queue: VecDeque<PathBuf>,
    pub sink: Option<Sink>,
    pub stream_handle: rodio::OutputStreamHandle,
    pub paused: bool,
    pub volume: f32,
    pub current_file: Option<PathBuf>,
    // Historique des pistes jouées pour la navigation précédente
    pub history: VecDeque<PathBuf>,
    // Gestionnaire de normalisation partagé
    pub normalization_manager: Arc<NormalizationManager>,
    pub normalization_enabled: bool,
}

impl fmt::Debug for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerState")
            .field("queue_len", &self.queue.len())
            .field("has_sink", &self.sink.is_some())
            .field("paused", &self.paused)
            .field("volume", &self.volume)
            .field(
                "current_file",
                &self.current_file.as_ref().and_then(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str().map(|s| s.to_string()))
                }),
            )
            .field("history_len", &self.history.len())
            .field("normalization_enabled", &self.normalization_enabled)
            .finish()
    }
}

impl PlayerState {
    /// Create a new player state with an initial volume and normalization flag.
    pub fn new(
        stream_handle: rodio::OutputStreamHandle,
        initial_volume: f32,
        initial_normalization_enabled: bool,
    ) -> Self {
        Self {
            queue: VecDeque::new(),
            sink: None,
            stream_handle,
            paused: false,
            volume: initial_volume,
            current_file: None,
            history: VecDeque::new(),
            normalization_manager: Arc::new(NormalizationManager::default()),
            normalization_enabled: initial_normalization_enabled,
        }
    }

    /// Pause playback if there is an active sink/track.
    pub fn pause(&mut self) {
        if let Some(sink) = &self.sink {
            sink.pause();
            self.paused = true;
        }
    }

    /// Resume playback if it was previously paused.
    pub fn resume(&mut self) {
        if let Some(sink) = &self.sink {
            sink.play();
            self.paused = false;
        }
    }

    /// Play the next track in the queue. If the queue is empty but a current
    /// track exists, rebuild the playlist from the parent folder and continue.
    pub fn play_next(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }

        // Ajouter la piste actuelle à l'historique avant de passer à la suivante
        if let Some(current_file) = &self.current_file {
            self.history.push_back(current_file.clone());
        }

        if let Some(next_file) = self.queue.pop_front() {
            let file_name = next_file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Inconnu".to_string());

            log_info(format!("Lecture : {}", file_name));

            match File::open(&next_file) {
                Ok(file) => {
                    match Decoder::new(BufReader::new(file)) {
                        Ok(source) => {
                            match Sink::try_new(&self.stream_handle) {
                                Ok(sink) => {
                                    let normalization_gain = if self.normalization_enabled {
                                        self.normalization_manager.get_or_compute_gain(&next_file)
                                    } else {
                                        1.0
                                    };

                                    sink.set_volume(self.volume * normalization_gain);

                                    sink.append(source);
                                    self.sink = Some(sink);
                                    self.current_file = Some(next_file);
                                }
                                Err(e) => {
                                    log_error(format!("Erreur création Sink audio : {}", e));
                                    // On essaie de passer à la suivante si erreur technique ?
                                    // Pour l'instant on s'arrête pour éviter une boucle rapide infinie
                                }
                            }
                        }
                        Err(e) => {
                            log_error(format!("Erreur décodage fichier {:?} : {}", next_file, e));
                            // Fichier corrompu, on passe au suivant
                            self.play_next();
                        }
                    }
                }
                Err(e) => {
                    log_error(format!(
                        "Impossible d'ouvrir le fichier {:?} : {}",
                        next_file, e
                    ));
                    // Fichier introuvable, on passe au suivant
                    self.play_next();
                }
            }
        } else {
            // Si la playlist est vide, on la reconstitue avec les mêmes fichiers
            if let Some(current_file) = &self.current_file {
                log_info("Fin de playlist - rebouclage infini");
                // On récupère le dossier parent du fichier actuel pour reconstituer la playlist
                if let Some(parent) = current_file.parent() {
                    // On clone le chemin pour éviter le conflit d'emprunt
                    let parent_path = parent.to_path_buf();
                    // On libère l'emprunt de current_file avant d'appeler add_folder
                    let _ = current_file;
                    self.add_folder(&parent_path);
                    // On joue immédiatement la première musique de la nouvelle playlist
                    self.play_next();
                }
            } else {
                // println!("Playlist vide");
                self.sink = None;
                self.current_file = None;
            }
        }
    }

    /// Update the in-memory volume and persist it to `env.json` (key `VOLUME`).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        if let Some(sink) = &self.sink {
            sink.set_volume(volume);
        }
        // Mettre à jour uniquement la clé VOLUME dans env.json
        if let Err(e) = update_config_key("VOLUME", serde_json::json!(volume)) {
            log_error(format!(
                "Erreur lors de la mise à jour du volume dans env.json: {}",
                e
            ));
        }
    }

    /// Enable or disable EBUR128 normalization and persist the state to `env.json`.
    pub fn set_normalization_enabled(&mut self, enabled: bool) {
        self.normalization_enabled = enabled;
        if let Err(e) = update_config_key("normalizationEnabled", serde_json::json!(enabled)) {
            log_error(format!(
                "Erreur lors de la mise à jour de la normalisation dans env.json: {}",
                e
            ));
        }
    }

    /// Replace the queue with all audio files found under `folder`, shuffled.
    pub fn add_folder(&mut self, folder: &Path) {
        // On liste les fichiers audio dans folder
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                if let Some(ext) = e.path().extension() {
                    matches!(
                        ext.to_str().unwrap_or("").to_lowercase().as_str(),
                        "mp3" | "wav" | "flac"
                    )
                } else {
                    false
                }
            })
            .map(|e| e.into_path())
            .collect();

        if files.is_empty() {
            log_info(format!("Aucun fichier audio trouvé dans {:?}", folder));
            return;
        }

        // Mélanger la liste
        files.shuffle(&mut rand::thread_rng());

        self.queue = VecDeque::from(files);
    }

    /// Stop playback, clear the queue and reset the current track.
    pub fn stop(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.sink = None;
        self.queue.clear();
        self.current_file = None;
    }

    /// Return the file name of the current track, if any.
    pub fn get_current_music_name(&self) -> Option<String> {
        self.current_file
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
    }

    /// Read and return ID3/Vorbis metadata from the current file.
    /// Falls back gracefully to filename-only if tags are absent or unreadable.
    pub fn get_current_music_metadata(&self) -> Option<MusicMetadata> {
        let path = self.current_file.as_ref()?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let mut meta = MusicMetadata {
            filename,
            title: None,
            artist: None,
            album: None,
            date: None,
            cover_art: None,
        };

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Some(meta),
        };

        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension() {
            hint.with_extension(&ext.to_string_lossy());
        }

        let mut probed = match symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        ) {
            Ok(p) => p,
            Err(_) => return Some(meta),
        };

        // probed.metadata   : Option<Metadata<'_>> → .current() → Option<&MetadataRevision>
        // probed.format     : Metadata<'_>        → .current() → Option<&MetadataRevision>
        // Les deux doivent être stockés en bindings pour que les lifetimes tiennent.

        let probe_meta = probed.metadata.get();
        let probe_rev = probe_meta.as_ref().and_then(|m| m.current());

        let fmt_meta = probed.format.metadata();
        let format_rev = fmt_meta.current();

        for rev in [probe_rev, format_rev].into_iter().flatten() {
            if meta.title.is_some() && meta.artist.is_some() && meta.cover_art.is_some() {
                break;
            }
            Self::fill_from_revision(rev, &mut meta);
        }

        Some(meta)
    }

    fn fill_from_revision(
        rev: &symphonia::core::meta::MetadataRevision,
        meta: &mut MusicMetadata,
    ) {
        for tag in rev.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => {
                    meta.title.get_or_insert_with(|| tag.value.to_string());
                }
                Some(StandardTagKey::Artist) => {
                    meta.artist.get_or_insert_with(|| tag.value.to_string());
                }
                Some(StandardTagKey::Album) => {
                    meta.album.get_or_insert_with(|| tag.value.to_string());
                }
                Some(StandardTagKey::Date) => {
                    meta.date.get_or_insert_with(|| tag.value.to_string());
                }
                _ => {}
            }
        }
        if meta.cover_art.is_none() {
            if let Some(visual) = rev.visuals().first() {
                let encoded = STANDARD.encode(&*visual.data);
                meta.cover_art = Some(format!("data:{};base64,{}", visual.media_type, encoded));
            }
        }
    }

    // Méthode pour jouer la piste précédente
    /// Play the previous track using the history and push the current one
    /// back to the front of the queue.
    pub fn play_previous(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }

        // Retirer la piste actuelle de l'historique et la remettre dans la queue
        if let Some(current_file) = &self.current_file {
            self.queue.push_front(current_file.clone());
        }

        // Récupérer la piste précédente depuis l'historique
        if let Some(previous_file) = self.history.pop_back() {
            let file_name = previous_file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Inconnu".to_string());

            log_info(format!("Lecture précédente : {}", file_name));

            match File::open(&previous_file) {
                Ok(file) => {
                    match Decoder::new(BufReader::new(file)) {
                        Ok(source) => match Sink::try_new(&self.stream_handle) {
                            Ok(sink) => {
                                let normalization_gain = if self.normalization_enabled {
                                    self.normalization_manager
                                        .get_or_compute_gain(&previous_file)
                                } else {
                                    1.0
                                };

                                sink.set_volume(self.volume * normalization_gain);

                                sink.append(source);
                                self.sink = Some(sink);
                                self.current_file = Some(previous_file);
                            }
                            Err(e) => {
                                log_error(format!("Erreur création Sink audio (prev) : {}", e));
                            }
                        },
                        Err(e) => {
                            log_error(format!(
                                "Erreur décodage fichier {:?} : {}",
                                previous_file, e
                            ));
                            // Si erreur, on tente encore la précédente ? Pour l'instant on stop.
                        }
                    }
                }
                Err(e) => {
                    log_error(format!(
                        "Impossible d'ouvrir le fichier {:?} : {}",
                        previous_file, e
                    ));
                }
            }
        } else {
            log_info("Aucune piste précédente disponible");
        }
    }

    // Méthode pour vérifier s'il y a une piste précédente
    /// Whether there is a previous track available in history.
    pub fn has_previous(&self) -> bool {
        !self.history.is_empty()
    }

    // Méthode pour obtenir le nom de la piste précédente
    /// Return the file name of the previous track, if available.
    pub fn get_previous_music_name(&self) -> Option<String> {
        self.history
            .back()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
    }
}

/// Return the list of folders directly under `./public/music`.
pub fn get_folders_list() -> Vec<String> {
    let music_path = Path::new("./public/music");
    let mut folders = Vec::new();
    if music_path.exists() && music_path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(music_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        folders.push(name.to_string());
                    }
                }
            }
        }
    }
    folders
}
