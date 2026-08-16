//! Background services: audio initialization and auto-play loop.

use rodio::OutputStream;
use std::sync::{Arc, Mutex};
use tokio;

use crate::model::PlayerState;

/// Service layer encapsulating shared access to `PlayerState` and related tasks.
pub struct PlayerService {
    player: Arc<Mutex<PlayerState>>,
}

impl PlayerService {
    /// Create a new service bound to a shared `PlayerState`.
    pub fn new(player: Arc<Mutex<PlayerState>>) -> Self {
        Self { player }
    }

    /// Start a background async task that watches track completion and auto-advances.
    pub fn start_auto_play_thread(&self) {
        let player_clone = self.player.clone();

        tokio::spawn(async move {
            loop {
                {
                    let mut p = player_clone.lock().unwrap();
                    // S'il n'y a pas de musique en cours, jouer la suivante
                    if p.sink.is_none() {
                        p.play_next();
                    } else if let Some(sink) = &p.sink {
                        // Si la musique est terminée
                        if sink.empty() && !p.paused {
                            p.play_next();
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        });
    }

    /// Initialize the default audio output and return both the stream and its handle.
    pub fn initialize_audio()
    -> Result<(OutputStream, rodio::OutputStreamHandle), rodio::StreamError> {
        OutputStream::try_default()
    }

    /// Set the console title on Windows.
    pub fn set_console_title() {
        janus_nucleus::console::set_title("JanusCore Server");
    }
}
