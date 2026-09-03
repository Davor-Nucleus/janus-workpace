//! État du lecteur : l'adaptateur carte son, autour du domaine partagé.
//!
//! L'ordre des pistes, l'historique et le rebouclage vivent dans
//! [`janus_playlist_nucleus::Playlist`], commun avec la diffusion en flux. Ne
//! restent ici que la sortie `rodio` et les trois réglages propres au serveur :
//! volume (clé `VOLUME`), pause, normalisation.

use std::path::Path;
use std::sync::Arc;
use std::{fmt, fs::File, io::BufReader};

use janus_config_nucleus::update_config_key;
use janus_library_nucleus::audio::NormalizationManager;
use janus_log_nucleus::{log_error, log_info};
use janus_playlist_nucleus::{
    next_playable, resolve_gain, warm_next, Playlist, TrackSink,
};
use rodio::{Decoder, Sink};
use serde::Deserialize;

pub use janus_library_nucleus::metadata::MusicMetadata;
pub use janus_playlist_nucleus::folders as get_folders_list;

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

/// La sortie de JanusCore : un `rodio::Sink` sur le périphérique par défaut.
///
/// Garde le gain de normalisation de la piste courante **à part** du volume.
/// Les multiplier une fois pour toutes à l'ouverture était le défaut de la version
/// précédente : régler le volume en cours de piste écrasait le produit et faisait
/// disparaître la normalisation jusqu'au morceau suivant.
pub struct RodioSink {
    stream_handle: rodio::OutputStreamHandle,
    sink: Option<Sink>,
    /// Gain de normalisation de la piste en cours.
    gain: f32,
    volume: f32,
}

impl RodioSink {
    pub fn new(stream_handle: rodio::OutputStreamHandle, volume: f32) -> Self {
        Self {
            stream_handle,
            sink: None,
            gain: 1.0,
            volume,
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.apply();
    }

    fn apply(&self) {
        if let Some(sink) = &self.sink {
            sink.set_volume(self.volume * self.gain);
        }
    }

    pub fn pause(&self) {
        if let Some(sink) = &self.sink {
            sink.pause();
        }
    }

    pub fn resume(&self) {
        if let Some(sink) = &self.sink {
            sink.play();
        }
    }

    /// Une piste est-elle chargée ? Distinct de [`TrackSink::is_exhausted`], qui
    /// est vrai aussi quand la piste est arrivée à son terme.
    pub fn has_track(&self) -> bool {
        self.sink.is_some()
    }
}

impl TrackSink for RodioSink {
    fn open(&mut self, path: &Path, gain: f32) -> bool {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                log_error(format!("Impossible d'ouvrir le fichier {path:?} : {e}"));
                return false;
            }
        };

        let source = match Decoder::new(BufReader::new(file)) {
            Ok(s) => s,
            Err(e) => {
                log_error(format!("Erreur décodage fichier {path:?} : {e}"));
                return false;
            }
        };

        let sink = match Sink::try_new(&self.stream_handle) {
            Ok(s) => s,
            Err(e) => {
                log_error(format!("Erreur création Sink audio : {e}"));
                return false;
            }
        };

        log_info(format!(
            "Lecture : {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));

        sink.append(source);
        self.gain = gain;
        self.sink = Some(sink);
        self.apply();
        true
    }

    fn is_exhausted(&self) -> bool {
        self.sink.as_ref().map_or(true, |s| s.empty())
    }

    fn clear(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.sink = None;
        self.gain = 1.0;
    }
}

/// État du lecteur : playlist partagée, sortie carte son, réglages locaux.
pub struct PlayerState {
    pub playlist: Playlist,
    pub sink: RodioSink,
    pub paused: bool,
    pub volume: f32,
    pub normalization_manager: Arc<NormalizationManager>,
    pub normalization_enabled: bool,
}

impl fmt::Debug for PlayerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerState")
            .field("queue_len", &self.playlist.queue_len())
            .field("has_sink", &self.sink.has_track())
            .field("paused", &self.paused)
            .field("volume", &self.volume)
            .field("current_file", &self.playlist.current_music_name())
            .field("history_len", &self.playlist.history_len())
            .field("normalization_enabled", &self.normalization_enabled)
            .finish()
    }
}

impl PlayerState {
    pub fn new(
        stream_handle: rodio::OutputStreamHandle,
        initial_volume: f32,
        initial_normalization_enabled: bool,
    ) -> Self {
        Self {
            playlist: Playlist::new(),
            sink: RodioSink::new(stream_handle, initial_volume),
            paused: false,
            volume: initial_volume,
            normalization_manager: Arc::new(NormalizationManager::default()),
            normalization_enabled: initial_normalization_enabled,
        }
    }

    pub fn pause(&mut self) {
        if self.sink.has_track() {
            self.sink.pause();
            self.paused = true;
        }
    }

    pub fn resume(&mut self) {
        if self.sink.has_track() {
            self.sink.resume();
            self.paused = false;
        }
    }

    /// Ouvre la piste suivante. `false` quand il n'y a plus rien à jouer.
    ///
    /// Le plafond de tentatives vit dans [`next_playable`] : un dossier entièrement
    /// corrompu rend la main au lieu de dérouler la pile, comme le faisait la
    /// version récursive.
    pub fn play_next(&mut self) -> bool {
        let enabled = self.normalization_enabled;
        let manager = Arc::clone(&self.normalization_manager);

        let Self { playlist, sink, .. } = self;
        let opened = next_playable(
            || playlist.next_track(),
            sink,
            |path| resolve_gain(path, enabled, &manager),
        );

        if opened {
            // Préchauffe la piste suivante pour que la transition soit normalisée.
            warm_next(&manager, enabled, playlist.peek_next().map(|p| p.as_path()));
            self.paused = false;
        } else {
            self.sink.clear();
        }
        opened
    }

    /// Revient à la piste précédente. `false` si l'historique est vide.
    pub fn play_previous(&mut self) -> bool {
        if !self.playlist.play_previous() {
            log_info("Aucune piste précédente disponible".to_string());
            return false;
        }
        self.play_next()
    }

    /// Update the in-memory volume and persist it to `env.json` (key `VOLUME`).
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.sink.set_volume(volume);
        if let Err(e) = update_config_key("VOLUME", serde_json::json!(volume)) {
            log_error(format!(
                "Erreur lors de la mise à jour du volume dans env.json: {e}"
            ));
        }
    }

    /// Enable or disable EBUR128 normalization and persist the state to `env.json`.
    pub fn set_normalization_enabled(&mut self, enabled: bool) {
        self.normalization_enabled = enabled;
        if let Err(e) = update_config_key("normalizationEnabled", serde_json::json!(enabled)) {
            log_error(format!(
                "Erreur lors de la mise à jour de la normalisation dans env.json: {e}"
            ));
        }
    }

    /// Stop playback, clear the queue and reset the current track.
    pub fn stop(&mut self) {
        self.sink.clear();
        self.playlist.clear();
        self.paused = false;
    }

    pub fn get_current_music_name(&self) -> Option<String> {
        self.playlist.current_music_name()
    }

    /// Read and return ID3/Vorbis metadata from the current file.
    pub fn get_current_music_metadata(&self) -> Option<MusicMetadata> {
        janus_library_nucleus::metadata::read_metadata(&self.playlist.current_path()?)
    }

    pub fn has_previous(&self) -> bool {
        self.playlist.has_previous()
    }

    pub fn get_previous_music_name(&self) -> Option<String> {
        self.playlist.previous_music_name()
    }
}
