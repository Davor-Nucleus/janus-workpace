use reqwest;
use rodio::{Decoder, Sink};
use serde_json;
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::Path,
    sync::{Arc, Mutex, mpsc},
};
use warp::Reply;

use crate::model::PlayerState;
use janus_common::logger::{log_error, log_info};

pub struct PlayerController;

impl PlayerController {
    pub async fn handle_soundboard_play(
        params: HashMap<String, String>,
        player: Arc<Mutex<PlayerState>>,
        music_port: u16,
    ) -> Result<impl Reply, std::convert::Infallible> {
        // 1. Vérifier l'état de la musique (paused ou playing ?)
        let status_url = format!("http://127.0.0.1:{}/api/status", music_port);
        let was_playing = match reqwest::get(&status_url).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    // Si paused == false, alors c'était en lecture
                    json.get("paused")
                        .and_then(|v| v.as_bool())
                        .map(|p| !p)
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            Err(_) => false, // Si on ne peut pas joindre JanusCore, on assume qu'il ne joue pas
        };

        // 2. Si c'était en lecture, on force la pause
        if was_playing {
            let _ = reqwest::get(format!("http://127.0.0.1:{}/api/pause", music_port)).await;
        }

        if let Some(sound_name) = params.get("sound") {
            let base = Path::new("./public/soundboard");
            let mut found = false;
            let mut file_path = base.join(sound_name);
            // Si l'utilisateur n'a pas mis d'extension, on tente mp3, wav, flac
            if !file_path.exists() {
                for ext in ["mp3", "wav", "flac"] {
                    let try_path = base.join(format!("{}.{}", sound_name, ext));
                    if try_path.exists() {
                        file_path = try_path;
                        found = true;
                        break;
                    }
                }
            } else {
                found = true;
            }
            if !found || !file_path.is_file() {
                return Ok(warp::reply::with_status(
                    format!("Son {:?} introuvable dans ./public/soundboard", sound_name),
                    warp::http::StatusCode::BAD_REQUEST,
                ));
            }
            // Joue le son du soundboard (via tokio spawn_blocking pour ne pas bloquer le runtime)
            tokio::task::spawn_blocking({
                let file_path = file_path.clone();
                let player = player.clone();
                let music_port = music_port;
                move || {

                    let is_active_normalization = false;

                    // Calculer le gain de normalisation (peut prendre un peu de temps au premier scan)
                    // Normalisation désactivée pour PhonosCore :
                    // on joue désormais les sons à leur volume brut (pas de EBU R128 ici).
                    let normalization_gain = {
                        if let Ok(p) = player.lock() && is_active_normalization {
                            p.normalization_manager.get_or_compute_gain(&file_path)
                        } else {
                            1.0
                        }
                    };

                    

                    // Récupérer le stream_handle du player principal
                    let stream_handle = {
                        if let Ok(p) = player.lock() {
                            p.stream_handle.clone()
                        } else {
                            return;
                        }
                    };

                    let file = match File::open(&file_path) {
                        Ok(f) => f,
                        Err(_) => return,
                    };
                    let source = match Decoder::new(BufReader::new(file)) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let sink = match Sink::try_new(&stream_handle) {
                        Ok(s) => s,
                        Err(_) => return,
                    };

                    // Récupérer le volume courant et l'appliquer au sink du soundboard
                    let current_volume = if let Ok(p) = player.lock() {
                        p.volume
                    } else {
                        1.0
                    };

                    // Appliquer le gain de normalisation
                    sink.set_volume(current_volume * normalization_gain);

                    // Créer un Arc<Mutex<Sink>> pour pouvoir le partager
                    let sink_arc = Arc::new(Mutex::new(sink));
                    let sink_arc_clone = sink_arc.clone();

                    // Créer un canal pour arrêter le thread
                    let (stop_sender, stop_receiver) = mpsc::channel();

                    // Ajouter le sink à la liste des sinks de la soundboard
                    if let Ok(p) = player.lock() {
                        log_info("Ajout du sink à la liste des sinks du soundboard");
                        p.add_soundboard_sink(sink_arc, stop_sender);
                    } else {
                        log_error("Impossible de verrouiller le player pour ajouter le sink");
                    }

                    // Utiliser le clone pour append
                    if let Ok(sink) = sink_arc_clone.lock() {
                        sink.append(source);
                    }

                    // Attendre la fin du son ou un signal d'arrêt (sans timeout)
                    let should_stop = {
                        loop {
                            // Vérifier le canal d'arrêt
                            if let Ok(_) = stop_receiver.try_recv() {
                                log_info("Signal d'arrêt reçu, arrêt du son");
                                if let Ok(sink) = sink_arc_clone.lock() {
                                    sink.stop();
                                }
                                break true;
                            }

                            // Vérifier si le son est terminé
                            let is_empty = if let Ok(sink) = sink_arc_clone.lock() {
                                sink.empty()
                            } else {
                                true
                            };
                            if is_empty {
                                log_info("Son terminé naturellement");
                                break false;
                            }

                            // Attendre un peu avant de vérifier à nouveau
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    };

                    // Retirer le sink de la liste des sinks actifs
                    if let Ok(p) = player.lock() {
                        p.remove_soundboard_sink(&sink_arc_clone);
                    }

                    // 4. Relance la musique seulement SI elle était en lecture AVANT
                    if !should_stop && was_playing {
                        let _ = reqwest::blocking::get(format!(
                            "http://127.0.0.1:{}/api/resume",
                            music_port
                        ));
                    }
                }
            });
            Ok(warp::reply::with_status(
                format!("Son {:?} joué", file_path.file_name().unwrap_or_default()),
                warp::http::StatusCode::OK,
            ))
        } else {
            Ok(warp::reply::with_status(
                "Paramètre 'sound' manquant".to_string(),
                warp::http::StatusCode::BAD_REQUEST,
            ))
        }
    }

    pub async fn handle_soundboard_sounds() -> Result<impl Reply, std::convert::Infallible> {
        let base = Path::new("./public/soundboard");

        if !base.exists() || !base.is_dir() {
            let response = serde_json::json!({
                "error": "Le dossier soundboard n'existe pas"
            });
            return Ok(warp::reply::with_status(
                warp::reply::json(&response),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }

        let mut sounds = Vec::new();

        match std::fs::read_dir(base) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() {
                            // Vérifie si c'est un fichier audio supporté
                            if let Some(extension) = path.extension() {
                                if let Some(ext_str) = extension.to_str() {
                                    if ["mp3", "wav", "flac", "ogg", "m4a"]
                                        .contains(&ext_str.to_lowercase().as_str())
                                    {
                                        if let Some(file_name) = path.file_name() {
                                            if let Some(name_str) = file_name.to_str() {
                                                sounds.push(name_str.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let response = serde_json::json!({ "sounds": sounds });
                Ok(warp::reply::with_status(
                    warp::reply::json(&response),
                    warp::http::StatusCode::OK,
                ))
            }
            Err(e) => {
                let response = serde_json::json!({
                    "error": format!("Erreur lors de la lecture du dossier soundboard: {}", e)
                });
                Ok(warp::reply::with_status(
                    warp::reply::json(&response),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                ))
            }
        }
    }

    pub async fn handle_soundboard_stop(
        player: Arc<Mutex<PlayerState>>,
        music_port: u16,
    ) -> Result<impl Reply, std::convert::Infallible> {
        log_info("API /api/soundboard/stop appelée");

        // Arrêter tous les sinks de la soundboard
        if let Ok(p) = player.lock() {
            log_info("Player verrouillé, arrêt des sinks...");
            p.stop_all_soundboard_sinks();
        } else {
            log_error("Impossible de verrouiller le player");
        }

        // Reprend la musique (force resume) sur l'autre programme
        let _ = reqwest::get(format!("http://127.0.0.1:{}/api/resume", music_port)).await;

        let response = serde_json::json!({
            "message": "Soundboard arrêtée avec succès"
        });
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::OK,
        ))
    }
}
