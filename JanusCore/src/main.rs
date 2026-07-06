//! Entry point for the headless MP3 server.
mod controller;
mod model;
mod routes;
mod service;

use std::sync::{Arc, Mutex};
use warp;

use crate::model::PlayerState;
use crate::routes::create_routes;
use crate::service::PlayerService;
use janus_nucleus::config::read_config;
use janus_nucleus::gui::LogWindowHandle;
use janus_nucleus::logger::{log_info, set_gui_enabled};

use warp::Filter;

/// Initialize audio and start the Warp HTTP server with CORS enabled.
#[tokio::main]
async fn main() {
    // Configuration initiale
    PlayerService::set_console_title();
    // Lecture de la configuration depios janus_nucleus
    let config = read_config();
    let initial_volume = config.volume.unwrap_or(1.0);
    let initial_normalization_enabled = config.normalization_enabled.unwrap_or(true);
    let port = config.port_music.unwrap_or(3030);
    let gui_enabled = config.janus_core_gui.unwrap_or(true);
    set_gui_enabled(gui_enabled);

    // Démarrer une petite fenêtre de logs et écouter sa fermeture si activée
    let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
    if gui_enabled {
        let log_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        LogWindowHandle::spawn(
            log_buffer,
            close_tx,
            "JanusCore - Logs".to_string(),
            "JanusCoreLogWndClass".to_string(),
        );
    }

    // initial_volume et port déjà lus ci-dessus

    // Initialisation audio
    let (_stream, stream_handle) = match PlayerService::initialize_audio() {
        Ok(s) => s,
        Err(e) => {
            janus_nucleus::logger::log_error(format!("Impossible d'initialiser l'audio : {}", e));
            return;
        }
    };
    let player = Arc::new(Mutex::new(PlayerState::new(
        stream_handle,
        initial_volume,
        initial_normalization_enabled,
    )));

    // Démarrage du service de lecture automatique
    let player_service = PlayerService::new(player.clone());
    player_service.start_auto_play_thread();

    // Création des routes
    let routes = create_routes(player);

    // Ajout du support CORS
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    let msg = format!("Serveur démarré sur http://127.0.0.1:{}", port);
    log_info(msg);
    // Lancer le serveur Warp en tâche de fond
    let (srv_tx, srv_rx) = tokio::sync::oneshot::channel::<()>();
    let server = warp::serve(routes.with(cors));
    let (_, server_fut) = server.bind_with_graceful_shutdown(([127, 0, 0, 1], port), async move {
        // Attendre le signal d'arrêt
        let _ = srv_rx.await;
    });

    let server_task = tokio::spawn(server_fut);

    // Bloquer jusqu'à fermeture de la fenêtre
    // (canal std::sync::mpsc est bloquant; le recevoir côté tokio est OK si peu fréquent)
    let _ = close_rx.recv();
    let _ = srv_tx.send(());
    let _ = server_task.await;
}
