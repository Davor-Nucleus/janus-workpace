//! Point d'entrée d'OrpheusCore : génération continue de synthwave, diffusée en MP3.
//!
//! Le seul serveur du workspace qui ne lit aucun fichier audio : tout est
//! synthétisé échantillon par échantillon. Comme WebRadioCore, il n'ouvre aucun
//! périphérique de sortie — le `Pacer` remplace la carte son pour cadencer la
//! production.

mod compose;
mod controller;
mod engine;
mod model;
mod render;
mod routes;
mod synth;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use warp::Filter;

use janus_nucleus::config::{parse_bind, read_config};
use janus_nucleus::logger::{log_error, log_info, set_gui_enabled};
use janus_nucleus::stream::{StreamHub, DEFAULT_LEAD};

use crate::compose::arranger::Arranger;
use crate::controller::OrpheusContext;
use crate::model::OrpheusState;
use crate::routes::create_routes;

#[tokio::main]
async fn main() {
    let config = read_config();

    let port = config.port_orpheus.unwrap_or(3006);
    let volume = config.orpheus_volume.unwrap_or(0.8);
    let bitrate_kbps = config.orpheus_bitrate.unwrap_or(192);
    let gui_enabled = config.orpheus_core_gui.unwrap_or(true);
    let base_bpm = config.orpheus_bpm.unwrap_or(92) as f32;
    // Graine absente d'`env.json` : chaque lancement produit une musique
    // différente. Fixée : la session se rejoue à l'identique.
    let seed = config.orpheus_seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED)
    });
    let allowed_origins = config.cors_origins();

    let bind_setting = config.orpheus_bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let bind_ip = parse_bind(&bind_setting, "orpheusBind");

    set_gui_enabled(gui_enabled);
    janus_nucleus::console::set_title("OrpheusCore Server");

    let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
    if gui_enabled {
        let log_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        janus_nucleus::gui::LogWindowHandle::spawn(
            log_buffer,
            close_tx,
            "OrpheusCore - Logs".to_string(),
            "OrpheusCoreLogWndClass".to_string(),
        );
    }

    // Un arrangeur jetable, uniquement pour disposer d'un instantané cohérent avant
    // que le moteur n'ait produit son premier bloc.
    let snapshot = Arranger::new(
        janus_nucleus::stream::OUTPUT_SAMPLE_RATE as f32,
        base_bpm,
        seed,
    )
    .snapshot();

    let state = Arc::new(Mutex::new(OrpheusState::new(volume, snapshot, seed)));
    let hub = Arc::new(StreamHub::default());
    let shutdown = Arc::new(AtomicBool::new(false));

    let engine = match engine::spawn(
        Arc::clone(&state),
        Arc::clone(&hub),
        bitrate_kbps,
        base_bpm,
        seed,
        DEFAULT_LEAD,
        Arc::clone(&shutdown),
    ) {
        Ok(handle) => handle,
        Err(e) => {
            log_error(format!("Démarrage du moteur impossible : {e}"));
            return;
        }
    };

    let ctx = Arc::new(OrpheusContext {
        state,
        hub,
        bitrate_kbps,
    });

    let cors = warp::cors()
        .allow_origins(allowed_origins.iter().map(String::as_str))
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    log_info(format!(
        "OrpheusCore démarré — flux sur http://{bind_ip}:{port}/stream.mp3 \
         (graine {seed}, {base_bpm} BPM de consigne, {bitrate_kbps} kbps)"
    ));
    if !bind_ip.is_loopback() {
        log_info(
            "Écoute sur le réseau local : le flux et l'API de contrôle sont \
             joignables depuis n'importe quelle machine du réseau. Passer \
             orpheusBind à 127.0.0.1 dans env.json pour les fermer."
                .to_string(),
        );
    }

    let (srv_tx, srv_rx) = tokio::sync::oneshot::channel::<()>();
    let (_, server_fut) = warp::serve(create_routes(ctx).with(cors))
        .bind_with_graceful_shutdown((bind_ip, port), async move {
            let _ = srv_rx.await;
        });
    let server_task = tokio::spawn(server_fut);

    // Sans GUI, `close_tx` est libéré aussitôt et `recv` rendrait la main
    // immédiatement : on attend alors Ctrl+C.
    if gui_enabled {
        let _ = close_rx.recv();
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }

    log_info("Arrêt en cours…".to_string());
    shutdown.store(true, Ordering::Relaxed);
    let _ = srv_tx.send(());
    let _ = server_task.await;
    let _ = engine.join();
}
