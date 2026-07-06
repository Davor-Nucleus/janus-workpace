//! Entry point for the headless MP3 server.
mod controller;
mod model;
mod routes;
mod service;
mod view;

use std::sync::{Arc, Mutex};
use warp;

use crate::model::{PlayerState, read_env_config};
use crate::routes::create_routes;
use crate::service::PlayerService;
use janus_common::gui::LogWindowHandle;
use janus_common::logger::{log_error, log_info, set_gui_enabled};

use warp::Filter;

/// Initialize audio and start the Warp HTTP server with CORS enabled.
#[tokio::main]
async fn main() {
    // Configuration initiale
    PlayerService::set_console_title();
    // Lecture de la configuration depuis env.json
    let (initial_volume, initial_limiter_db, port, gui_enabled) = match read_env_config() {
        Ok(cfg) => (
            cfg.volume.unwrap_or(1.0),
            cfg.limiter_db.unwrap_or(0.0),
            cfg.port_music.unwrap_or(3030),
            cfg.janus_core_gui.unwrap_or(true),
        ),
        Err(_) => (1.0, 0.0, 3030, true),
    };
    set_gui_enabled(gui_enabled);

    // Démarrer une petite fenêtre de logs et écouter sa fermeture si activée
    let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
    if gui_enabled {
        let log_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        LogWindowHandle::spawn(log_buffer, close_tx);
    }

    // initial_volume et port déjà lus ci-dessus

    // Initialisation audio
    let (_stream, stream_handle) = PlayerService::initialize_audio();
    let player = Arc::new(Mutex::new(PlayerState::new(
        stream_handle,
        initial_volume,
        initial_limiter_db,
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
