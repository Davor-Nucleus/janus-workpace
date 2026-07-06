//! Core data model and playback logic for the headless MP3 server.
//! Exposes configuration types (`EnvConfig`, `VolumeRequest`) and the player state (`PlayerState`).
//! Also provides helpers to read/write `env.json` and to discover music folders.

use janus_common::logger::{log_error, log_info};
use rand::seq::SliceRandom;
use rodio::{Decoder, Sink};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::{
    collections::VecDeque,
    fmt,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
/// Configuration read from `env.json`.
pub struct EnvConfig {
    #[serde(rename = "PORT_MUSIC")]
    pub port_music: Option<u16>,
    #[serde(rename = "VOLUME")]
    pub volume: Option<f32>,
    #[serde(rename = "LIMITER_DB")]
    pub limiter_db: Option<f32>,
    #[serde(rename = "janusCoreGui")]
    pub janus_core_gui: Option<bool>,
}

#[derive(Deserialize)]
/// JSON request body for updating the player volume.
pub struct VolumeRequest {
    pub volume: f32,
}

#[derive(Deserialize)]
/// JSON request body for updating the limiter.
pub struct LimiterRequest {
    pub limiter_db: f32,
}

/// Represents the current state of the player: queue, current track,
/// volume and pause state, plus a bounded history to navigate backwards.

pub struct PlayerState {
    pub queue: VecDeque<PathBuf>,
    pub sink: Option<Sink>,
    pub stream_handle: rodio::OutputStreamHandle,
    pub paused: bool,
    pub volume: f32,
    pub limiter_db: f32,
    pub current_file: Option<PathBuf>,
    // Historique des pistes jouées pour la navigation précédente
    pub history: VecDeque<PathBuf>,
}

impl fmt::Debug for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerState")
            .field("queue_len", &self.queue.len())
            .field("has_sink", &self.sink.is_some())
            .field("paused", &self.paused)
            .field("volume", &self.volume)
            .field("limiter_db", &self.limiter_db)
            .field(
                "current_file",
                &self.current_file.as_ref().and_then(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str().map(|s| s.to_string()))
                }),
            )
            .field("history_len", &self.history.len())
            .finish()
    }
}

impl PlayerState {
    /// Create a new player state with an initial volume and limiter.
    pub fn new(
        stream_handle: rodio::OutputStreamHandle,
        initial_volume: f32,
        initial_limiter_db: f32,
    ) -> Self {
        Self {
            queue: VecDeque::new(),
            sink: None,
            stream_handle,
            paused: false,
            volume: initial_volume,
            limiter_db: initial_limiter_db,
            current_file: None,
            history: VecDeque::new(),
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
            log_info(format!("Lecture : {:?}", next_file.file_name().unwrap()));

            let file = File::open(&next_file).unwrap();
            let source = Decoder::new(BufReader::new(file)).unwrap();
            let sink = Sink::try_new(&self.stream_handle).unwrap();
            let final_volume = self.calculate_final_volume();
            sink.set_volume(final_volume);
            sink.append(source);
            self.sink = Some(sink);
            self.current_file = Some(next_file);
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

    /// Calcule le volume final en appliquant le limiteur en dB
    fn calculate_final_volume(&self) -> f32 {
        // Convertir le volume linéaire en dB
        let volume_db = if self.volume > 0.0 {
            20.0 * self.volume.log10()
        } else {
            f32::NEG_INFINITY
        };

        // Si le volume en dB dépasse la limite, le réduire
        if volume_db > self.limiter_db {
            // Convertir la limite en dB en volume linéaire
            10.0_f32.powf(self.limiter_db / 20.0)
        } else {
            self.volume
        }
    }

    /// Update the in-memory volume and persist it to `env.json` (key `VOLUME`).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        let final_volume = self.calculate_final_volume();
        if let Some(sink) = &self.sink {
            sink.set_volume(final_volume);
        }
        // Mettre à jour uniquement la clé VOLUME dans env.json
        if let Err(e) = update_env_key("VOLUME", serde_json::json!(volume)) {
            log_error(format!(
                "Erreur lors de la mise à jour du volume dans env.json: {e}"
            ));
        }
    }

    /// Update the in-memory limiter and persist it to `env.json` (key `LIMITER_DB`).
    pub fn set_limiter(&mut self, limiter_db: f32) {
        self.limiter_db = limiter_db;
        let final_volume = self.calculate_final_volume();
        if let Some(sink) = &self.sink {
            sink.set_volume(final_volume);
        }
        // Mettre à jour uniquement la clé LIMITER_DB dans env.json
        if let Err(e) = update_env_key("LIMITER_DB", serde_json::json!(limiter_db)) {
            log_error(format!(
                "Erreur lors de la mise à jour du limiteur dans env.json: {e}"
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

    /// Toggle pause: pause if currently playing, resume if currently paused.
    pub fn toggle_pause(&mut self) {
        if self.paused {
            self.resume();
        } else {
            self.pause();
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
            log_info(format!(
                "Lecture précédente : {:?}",
                previous_file.file_name().unwrap()
            ));

            let file = File::open(&previous_file).unwrap();
            let source = Decoder::new(BufReader::new(file)).unwrap();
            let sink = Sink::try_new(&self.stream_handle).unwrap();
            let final_volume = self.calculate_final_volume();
            sink.set_volume(final_volume);
            sink.append(source);
            self.sink = Some(sink);
            self.current_file = Some(previous_file);
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

/// Read and deserialize `env.json` into an `EnvConfig`.
pub fn read_env_config() -> Result<EnvConfig, Box<dyn std::error::Error>> {
    let data = fs::read_to_string("env.json")?;
    let config: EnvConfig = serde_json::from_str(&data)?;
    Ok(config)
}

/// Serialize and write `EnvConfig` back to `env.json`.
pub fn write_env_config(config: &EnvConfig) -> Result<(), Box<dyn std::error::Error>> {
    let data = serde_json::to_string_pretty(config)?;
    fs::write("env.json", data)?;
    Ok(())
}

/// Update an arbitrary key in `env.json` with a given JSON value.
pub fn update_env_key(key: &str, value: Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut data: Value = serde_json::from_str(&fs::read_to_string("env.json")?)?;
    data[key] = value;
    fs::write("env.json", serde_json::to_string_pretty(&data)?)?;
    Ok(())
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
