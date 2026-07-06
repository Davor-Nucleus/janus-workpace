//! HTTP controllers (Warp handlers) orchestrating actions on the player state.

use futures::{SinkExt, StreamExt};
use serde_json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::sleep;
use warp::ws::Message;
use warp::ws::Ws;
use warp::{Rejection, Reply};

use crate::model::{LimiterRequest, PlayerState, VolumeRequest, get_folders_list};

/// Grouping of HTTP API handlers for the player.
pub struct PlayerController;

impl PlayerController {
    /// Start playback from a given folder (query parameter `folder`).
    /// Returns a status message if the folder is valid.
    pub async fn handle_folder(
        params: HashMap<String, String>,
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, Rejection> {
        if let Some(folder_name) = params.get("folder") {
            let base = Path::new("./public/music");
            let folder_path = base.join(folder_name);

            if !folder_path.exists() || !folder_path.is_dir() {
                return Ok(warp::reply::with_status(
                    format!("Le dossier {:?} n'existe pas", folder_path),
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }

            let mut p = player.lock().unwrap();
            p.add_folder(&folder_path);
            p.play_next();

            Ok(warp::reply::with_status(
                format!("Lecture du dossier {:?}", folder_path),
                warp::http::StatusCode::OK,
            ))
        } else {
            Ok(warp::reply::with_status(
                "Paramètre 'folder' manquant".to_string(),
                warp::http::StatusCode::BAD_REQUEST,
            ))
        }
    }

    /// Stop playback and clear the playlist.
    pub async fn handle_stop(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        p.stop();
        Ok(warp::reply::with_status(
            "Lecture stoppée et playlist vidée",
            warp::http::StatusCode::OK,
        ))
    }

    /// Force pause the playback.
    pub async fn handle_pause(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        p.pause();

        Ok(warp::reply::with_status(
            "Lecture mise en pause",
            warp::http::StatusCode::OK,
        ))
    }

    /// Force resume the playback.
    pub async fn handle_resume(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        p.resume();

        Ok(warp::reply::with_status(
            "Lecture reprise",
            warp::http::StatusCode::OK,
        ))
    }

    /// Return the list of folders located under `./public/music`.
    pub async fn handle_folderlist() -> Result<impl Reply, std::convert::Infallible> {
        let folders = get_folders_list();
        let response = serde_json::json!({ "folders": folders });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Return the current output volume as JSON.
    pub async fn handle_get_volume(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();
        let response = serde_json::json!({ "volume": p.volume });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Update the volume using a JSON body `{ volume: f32 }`.
    pub async fn handle_set_volume(
        req: VolumeRequest,
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        let mut new_volume = req.volume;
        // Clamp le volume entre 0.0 et 1.0
        if new_volume < 0.0 {
            new_volume = 0.0;
        }
        if new_volume > 1.0 {
            new_volume = 1.0;
        }
        p.set_volume(new_volume);
        let response = serde_json::json!({
            "message": "Volume mis à jour avec succès",
            "volume": p.volume
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Increase volume by +0.05 (clamped to [0.0, 1.0]).
    pub async fn handle_volume_add(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        let mut new_volume = p.volume + 0.05;
        // Clamp le volume entre 0.0 et 1.0
        if new_volume < 0.0 {
            new_volume = 0.0;
        }
        if new_volume > 1.0 {
            new_volume = 1.0;
        }
        p.set_volume(new_volume);
        let response = serde_json::json!({
            "message": "Volume mis à jour avec succès",
            "volume": p.volume
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Decrease volume by -0.05 (clamped to [0.0, 1.0]).
    pub async fn handle_volume_subtract(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        let mut new_volume = p.volume - 0.05;
        // Clamp le volume entre 0.0 et 1.0
        if new_volume < 0.0 {
            new_volume = 0.0;
        }
        if new_volume > 1.0 {
            new_volume = 1.0;
        }
        p.set_volume(new_volume);
        let response = serde_json::json!({
            "message": "Volume mis à jour avec succès",
            "volume": p.volume
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Return the current player status (paused, current music).
    pub async fn handle_status(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();
        let response = serde_json::json!({
            "paused": p.paused,
            "current_music": p.get_current_music_name(),
            "volume": p.volume
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Return metadata (title, artist, album, date, cover_art) of the currently playing track.
    pub async fn handle_current_music(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();
        if let Some(metadata) = p.get_current_music_metadata() {
            Ok(warp::reply::with_status(
                warp::reply::json(&metadata),
                warp::http::StatusCode::OK,
            ))
        } else {
            let response = serde_json::json!({ "message": "Aucune musique en cours de lecture." });
            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::OK,
            ))
        }
    }

    /// WebSocket handler to stream current music state updates.
    pub async fn handle_current_music_ws(
        ws: Ws,
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, Rejection> {
        Ok(ws.on_upgrade(move |socket| async move {
            let (mut tx, mut rx) = socket.split();

            tokio::spawn(async move {
                while let Some(_msg) = rx.next().await {}
            });

            let mut last: Option<String> = None;
            // Cache metadata par chemin : ne relit le fichier que lors d'un changement de piste
            let mut last_path: Option<PathBuf> = None;
            let mut cached_meta: serde_json::Value = serde_json::Value::Null;

            loop {
                let snapshot = {
                    let guard = player.lock().unwrap();
                    let current_path = guard.current_file.clone();

                    if current_path != last_path {
                        cached_meta = match guard.get_current_music_metadata() {
                            Some(m) => serde_json::to_value(&m).unwrap_or(serde_json::Value::Null),
                            None => serde_json::Value::Null,
                        };
                        last_path = current_path;
                    }

                    let has_next = !guard.queue.is_empty() || guard.current_file.is_some();
                    serde_json::json!({
                        "queue_len": guard.queue.len(),
                        "has_sink": guard.sink.is_some(),
                        "paused": guard.paused,
                        "volume": guard.volume,
                        "limiter_db": guard.limiter_db,
                        "current_music": guard.get_current_music_name(),
                        "has_next": has_next,
                        "history_len": guard.history.len(),
                        "metadata": cached_meta,
                    })
                    .to_string()
                };

                if last.as_ref().map(|s| s != &snapshot).unwrap_or(true) {
                    if tx.send(Message::text(snapshot.clone())).await.is_err() {
                        break;
                    }
                    last = Some(snapshot);
                }
                sleep(Duration::from_millis(500)).await;
            }
        }))
    }

    // Méthode pour jouer la piste précédente
    /// Play the previous track if available.
    pub async fn handle_previous(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();

        if p.has_previous() {
            p.play_previous();
            let music_name = p
                .get_current_music_name()
                .unwrap_or_else(|| "Inconnu".to_string());

            let response = serde_json::json!({
                "message": "Piste précédente jouée",
                "current_music": music_name
            });

            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::OK,
            ))
        } else {
            let response = serde_json::json!({
                "message": "Aucune piste précédente disponible",
                "error": "no_previous_track"
            });

            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::BAD_REQUEST,
            ))
        }
    }

    // Méthode pour vérifier s'il y a une piste précédente
    /// Indicate whether a previous track exists and return its name.
    pub async fn handle_has_previous(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();

        let response = serde_json::json!({
            "has_previous": p.has_previous(),
            "previous_music": p.get_previous_music_name()
        });

        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    // Méthode pour jouer la piste suivante
    /// Play the next track. If the queue is empty, attempt to rebuild the playlist.
    pub async fn handle_next(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();

        // Vérifier s'il y a une piste suivante dans la queue
        if !p.queue.is_empty() {
            p.play_next();
            let music_name = p
                .get_current_music_name()
                .unwrap_or_else(|| "Inconnu".to_string());

            let response = serde_json::json!({
                "message": "Piste suivante jouée",
                "current_music": music_name
            });

            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::OK,
            ))
        } else {
            // Si la queue est vide, on peut quand même essayer de jouer la suivante
            // (cela peut reconstituer la playlist)
            p.play_next();
            let music_name = p
                .get_current_music_name()
                .unwrap_or_else(|| "Inconnu".to_string());

            let response = serde_json::json!({
                "message": "Piste suivante jouée (playlist reconstituée)",
                "current_music": music_name
            });

            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::OK,
            ))
        }
    }

    // Méthode pour vérifier s'il y a une piste suivante
    /// Indicate whether a next track exists, along with queue length and current track.
    pub async fn handle_has_next(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();

        let has_next = !p.queue.is_empty() || p.current_file.is_some();

        let response = serde_json::json!({
            "has_next": has_next,
            "queue_length": p.queue.len(),
            "current_music": p.get_current_music_name()
        });

        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Return the current limiter value as JSON.
    pub async fn handle_get_limiter(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let p = player.lock().unwrap();
        let response = serde_json::json!({ "limiter": p.limiter_db });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Update the limiter using a JSON body `{ limiter_db: f32 }`.
    pub async fn handle_set_limiter(
        req: LimiterRequest,
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        p.set_limiter(req.limiter_db);
        let response = serde_json::json!({
            "message": "Limiteur mis à jour avec succès",
            "limiter": p.limiter_db
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Increase limiter by +1.0 dB.
    pub async fn handle_limiter_add(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        let new_limiter = p.limiter_db + 1.0;
        p.set_limiter(new_limiter);
        let response = serde_json::json!({
            "message": "Limiteur mis à jour avec succès",
            "limiter": p.limiter_db
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }

    /// Decrease limiter by -1.0 dB.
    pub async fn handle_limiter_subtract(
        player: Arc<Mutex<PlayerState>>,
    ) -> Result<impl Reply, std::convert::Infallible> {
        let mut p = player.lock().unwrap();
        let new_limiter = p.limiter_db - 1.0;
        p.set_limiter(new_limiter);
        let response = serde_json::json!({
            "message": "Limiteur mis à jour avec succès",
            "limiter": p.limiter_db
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }
}
