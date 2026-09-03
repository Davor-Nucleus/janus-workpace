//! État de la radio : les réglages propres au serveur, autour du domaine partagé.
//!
//! L'ordre des pistes, l'historique et le rebouclage vivent dans
//! [`janus_playlist_nucleus::Playlist`], commun avec la lecture sur carte son. Ne
//! restent ici que les trois réglages qui, eux, ne se partagent pas : le volume
//! (clé `WEBRADIO_VOLUME`, distincte de celle de la lecture locale), la pause, et
//! la normalisation.
//!
//! Toujours dépourvu de `Sink` et d'`OutputStreamHandle` : WebRadioCore n'ouvre
//! aucun périphérique audio, ce qui lui permet de tourner sur une machine qui n'en
//! a pas.

use std::sync::Arc;

use janus_config_nucleus::persist_key;
use janus_library_nucleus::audio::NormalizationManager;
use janus_playlist_nucleus::Playlist;

pub use janus_playlist_nucleus::{folders as get_folders_list, MUSIC_ROOT};

pub struct RadioState {
    playlist: Playlist,
    volume: f32,
    paused: bool,
    normalization_enabled: bool,
    normalization_manager: Arc<NormalizationManager>,
}

impl RadioState {
    pub fn new(volume: f32, normalization_enabled: bool) -> Self {
        Self {
            playlist: Playlist::new(),
            volume: volume.clamp(0.0, 1.0),
            paused: false,
            normalization_enabled,
            normalization_manager: Arc::new(NormalizationManager::default()),
        }
    }

    pub fn playlist(&self) -> &Playlist {
        &self.playlist
    }

    pub fn playlist_mut(&mut self) -> &mut Playlist {
        &mut self.playlist
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Règle le volume et le persiste dans `env.json`.
    ///
    /// Clé `WEBRADIO_VOLUME`, distincte de `VOLUME` que se partagent déjà JanusCore
    /// et PhonosCore : la radio est seule à l'écrire, donc aucun conflit d'écrivains.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        persist_key("WEBRADIO_VOLUME", serde_json::json!(self.volume));
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn normalization_enabled(&self) -> bool {
        self.normalization_enabled
    }

    pub fn normalization_manager(&self) -> Arc<NormalizationManager> {
        Arc::clone(&self.normalization_manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_volume_est_borne_a_la_construction() {
        assert_eq!(RadioState::new(5.0, false).volume(), 1.0);
        assert_eq!(RadioState::new(-1.0, false).volume(), 0.0);
    }

    #[test]
    fn la_pause_se_memorise() {
        let mut state = RadioState::new(1.0, false);
        assert!(!state.is_paused());
        state.set_paused(true);
        assert!(state.is_paused());
        state.set_paused(false);
        assert!(!state.is_paused());
    }
}
