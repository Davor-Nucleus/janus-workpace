mod controller;
mod model;
mod routes;
mod service;
mod view;

use std::sync::{Arc, Mutex, mpsc};
use warp;

use crate::model::{PlayerState, read_env_config};
use crate::routes::create_routes;
use crate::service::PlayerService;
use janus_common::logger::{log_error, log_info, set_global_log_buffer_ptr, set_gui_enabled};

use warp::Filter;

#[tokio::main]
async fn main() {
    // Configuration initiale
    PlayerService::set_console_title();

    // Lecture de la configuration depuis env.json
    let (initial_volume, port, gui_enabled_default_true) = match read_env_config() {
        Ok(cfg) => (
            cfg.volume.unwrap_or(1.0),
            cfg.port_soundboard.unwrap_or(3003),
            cfg.janus_core_gui.unwrap_or(true),
        ),
        Err(_) => (1.0, 3003, true),
    };
    set_gui_enabled(gui_enabled_default_true);

    // Préparer le buffer de logs global AVANT toute écriture de logs
    #[cfg(windows)]
    let mut precreated_log_buf: Option<Arc<Mutex<String>>> = None;
    #[cfg(windows)]
    if gui_enabled_default_true {
        let log_buf = Arc::new(Mutex::new(String::new()));
        let raw_ptr: *const Mutex<String> = Arc::into_raw(log_buf.clone());
        set_global_log_buffer_ptr(raw_ptr);
        precreated_log_buf = Some(log_buf);
    }

    // Initialisation audio
    let (_stream, stream_handle) = PlayerService::initialize_audio();
    let player = Arc::new(Mutex::new(PlayerState::new(stream_handle, initial_volume)));

    // Détecte le port du programme musique via variable d'environnement (fallback 3001)
    let music_port: u16 = std::env::var("PORT_MUSIC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3001);

    // Création des routes (soundboard uniquement)
    let routes = create_routes(player.clone(), music_port);

    // Ajout du support CORS
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    log_info(format!("Serveur démarré sur http://127.0.0.1:{}", port));

    // Si GUI activée, démarrer la fenêtre et écouter la fermeture
    #[cfg(windows)]
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    #[cfg(windows)]
    if gui_enabled_default_true {
        let log_buf = precreated_log_buf.expect("log buffer should be precreated when GUI enabled");
        janus_common::gui::LogWindowHandle::spawn(log_buf.clone(), shutdown_tx.clone());
    }

    // Warp graceful shutdown si GUI active
    #[cfg(windows)]
    if gui_enabled_default_true {
        let (_addr, server) = warp::serve(routes.with(cors)).bind_with_graceful_shutdown(
            ([127, 0, 0, 1], port),
            async move {
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = shutdown_rx.recv();
                })
                .await;
            },
        );
        server.await;
        return;
    }

    // Fallback: pas de GUI
    warp::serve(routes.with(cors))
        .run(([127, 0, 0, 1], port))
        .await;
}
