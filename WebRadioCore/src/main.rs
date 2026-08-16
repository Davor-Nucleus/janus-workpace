//! Point d'entrée de WebRadioCore : diffusion de la musique en MP3 sur HTTP.
//!
//! À la différence de JanusCore et PhonosCore, ce serveur **n'ouvre aucun
//! périphérique audio**. Il décode la musique à la cadence de l'horloge, l'encode
//! en MP3 et la distribue aux clients HTTP.

mod controller;
mod engine;
mod model;
mod routes;

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use warp::Filter;

use janus_nucleus::config::read_config;
use janus_nucleus::logger::{log_error, log_info, set_gui_enabled};
use janus_nucleus::stream::{StreamHub, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE};

use crate::controller::RadioContext;
use crate::model::RadioState;
use crate::routes::create_routes;

/// Avance du producteur sur l'horloge.
///
/// Autant d'audio déjà encodé et distribué en avance, qui donne au client de quoi
/// absorber la gigue du réseau sans couper. Payé en latence, sans importance pour
/// une radio.
const STREAM_LEAD: Duration = Duration::from_millis(1_500);

#[tokio::main]
async fn main() {
    let config = read_config();

    let port = config.port_webradio.unwrap_or(3005);
    let volume = config.webradio_volume.unwrap_or(1.0);
    let bitrate_kbps = config.webradio_bitrate.unwrap_or(192);
    let normalization = config.webradio_normalization.unwrap_or(true);
    let gui_enabled = config.webradio_core_gui.unwrap_or(true);
    // Lu avant `webradio_bind`, qui consomme la `String` et donc `config`.
    let allowed_origins = config.cors_origins();

    let bind_setting = config
        .webradio_bind
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let bind_ip: IpAddr = match bind_setting.parse() {
        Ok(ip) => ip,
        Err(_) => {
            // On se rabat sur la boucle locale, pas sur 0.0.0.0 : une coquille dans
            // la configuration ne doit pas exposer le serveur par accident.
            log_error(format!(
                "webRadioBind « {bind_setting} » illisible — repli sur 127.0.0.1"
            ));
            IpAddr::from([127, 0, 0, 1])
        }
    };

    set_gui_enabled(gui_enabled);
    janus_nucleus::console::set_title("WebRadioCore Server");

    let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
    if gui_enabled {
        let log_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        janus_nucleus::gui::LogWindowHandle::spawn(
            log_buffer,
            close_tx,
            "WebRadioCore - Logs".to_string(),
            "WebRadioCoreLogWndClass".to_string(),
        );
    }

    let state = Arc::new(Mutex::new(RadioState::new(volume, normalization)));
    let hub = Arc::new(StreamHub::default());
    let shutdown = Arc::new(AtomicBool::new(false));

    let engine = match engine::spawn(
        Arc::clone(&state),
        Arc::clone(&hub),
        bitrate_kbps,
        STREAM_LEAD,
        Arc::clone(&shutdown),
    ) {
        Ok(handle) => handle,
        Err(e) => {
            log_error(format!("Démarrage du moteur impossible : {e}"));
            return;
        }
    };

    let ctx = Arc::new(RadioContext {
        state,
        hub,
        bitrate_kbps,
    });

    // CORS restreint aux pages servies par praetorcast-core, comme JanusCore.
    // Ne protège que les navigateurs : voir l'avertissement sur l'écoute réseau
    // plus bas.
    let cors = warp::cors()
        .allow_origins(allowed_origins.iter().map(String::as_str))
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    log_info(format!(
        "WebRadioCore démarré — flux sur http://{bind_ip}:{port}/stream.mp3 \
         ({OUTPUT_SAMPLE_RATE} Hz, {OUTPUT_CHANNELS} canaux, {bitrate_kbps} kbps)"
    ));
    if !bind_ip.is_loopback() {
        log_info(
            "Écoute sur le réseau local : le flux et l'API de contrôle sont \
             joignables depuis n'importe quelle machine du réseau. Passer \
             webRadioBind à 127.0.0.1 dans env.json pour les fermer."
                .to_string(),
        );
    }

    let (srv_tx, srv_rx) = tokio::sync::oneshot::channel::<()>();
    let (_, server_fut) = warp::serve(create_routes(ctx).with(cors))
        .bind_with_graceful_shutdown((bind_ip, port), async move {
            let _ = srv_rx.await;
        });
    let server_task = tokio::spawn(server_fut);

    // Bloque jusqu'à la fermeture de la fenêtre de logs. Sans GUI, `close_tx` est
    // libéré immédiatement et `recv` rend la main aussitôt : on attend alors
    // Ctrl+C pour ne pas s'arrêter au démarrage.
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
