mod controller;
mod model;
mod routes;
mod service;

use std::sync::{Arc, Mutex, mpsc};
use warp;

use crate::model::PlayerState;
use crate::routes::create_routes;
use crate::service::PlayerService;
use janus_nucleus::config::read_config;
use janus_nucleus::logger::{log_info, set_global_log_buffer_ptr, set_gui_enabled};

use warp::Filter;

#[tokio::main]
async fn main() {
    // Configuration initiale
    PlayerService::set_console_title();

    // Lecture de la configuration depuis janus_nucleus
    let config = read_config();
    let initial_volume = config.volume.unwrap_or(1.0);
    let port = config.port_soundboard.unwrap_or(3003);
    let gui_enabled_default_true = config.phonos_core_gui.unwrap_or(true);
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
    let (_stream, stream_handle) = match PlayerService::initialize_audio() {
        Ok(s) => s,
        Err(e) => {
            janus_nucleus::logger::log_error(format!("Impossible d'initialiser l'audio : {}", e));
            return;
        }
    };
    let player = Arc::new(Mutex::new(PlayerState::new(stream_handle, initial_volume)));

    // Port de JanusCore, lu dans env.json comme le reste. Il passait par une variable
    // d'environnement que personne ne définit : la valeur retenue était toujours le
    // repli 3001, et changer PORT_MUSIC dans env.json cassait la pause automatique.
    let music_port = config.port_music.unwrap_or(3001);

    // Création des routes (soundboard uniquement)
    let routes = create_routes(player.clone(), music_port);

    // CORS restreint aux pages servies par praetorcast-core (cf. JanusCore).
    let core_port = config.port.unwrap_or(3000);
    let allowed_origins = [
        format!("http://localhost:{}", core_port),
        format!("http://127.0.0.1:{}", core_port),
    ];
    let cors = warp::cors()
        .allow_origins(allowed_origins.iter().map(String::as_str))
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    log_info(format!("Serveur démarré sur http://127.0.0.1:{}", port));

    // Si GUI activée, démarrer la fenêtre et écouter la fermeture
    #[cfg(windows)]
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    #[cfg(windows)]
    if gui_enabled_default_true {
        let log_buf =
            precreated_log_buf.expect("log buffer should be precreated when GUI enabled");
        janus_nucleus::gui::LogWindowHandle::spawn(
            log_buf.clone(),
            shutdown_tx.clone(),
            "PhonosCore - Logs".to_string(),
            "PhonosCoreLogWndClass".to_string(),
        );
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
