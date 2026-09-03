//! Handlers Warp d'OrpheusCore.

use serde::Deserialize;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use warp::http::StatusCode;
use warp::Reply;

use janus_log_nucleus::log_info;
use janus_stream_nucleus::{stream_response, StreamHub, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE};

use crate::model::OrpheusState;

#[derive(Deserialize)]
pub struct VolumeRequest {
    pub volume: f32,
}

/// Pas d'un cran de volume, aligné sur JanusCore.
const VOLUME_STEP: f32 = 0.05;

pub struct OrpheusContext {
    pub state: Arc<Mutex<OrpheusState>>,
    pub hub: Arc<StreamHub>,
    pub bitrate_kbps: u16,
}

pub struct OrpheusController;

impl OrpheusController {
    /// Le flux lui-même. Les en-têtes sont ceux de
    /// [`janus_stream_nucleus::stream_response`], communs aux serveurs de flux.
    pub async fn handle_stream(ctx: Arc<OrpheusContext>) -> Result<impl Reply, Infallible> {
        Ok(stream_response(&ctx.hub, "OrpheusCore", Some("Synthwave")))
    }

    /// État musical courant.
    pub async fn handle_status(ctx: Arc<OrpheusContext>) -> Result<impl Reply, Infallible> {
        let (snapshot, volume) = {
            let s = ctx.state.lock().unwrap();
            (s.snapshot().clone(), s.volume())
        };

        Ok(warp::reply::json(&serde_json::json!({
            "bpm": snapshot.bpm,
            "key": snapshot.key,
            "chord": snapshot.chord,
            "bar": snapshot.bar,
            "section": snapshot.section,
            "energy": snapshot.energy,
            "seed": snapshot.seed,
            "volume": volume,
            "listeners": ctx.hub.listener_count(),
            "bitrate_kbps": ctx.bitrate_kbps,
            "sample_rate": OUTPUT_SAMPLE_RATE,
            "channels": OUTPUT_CHANNELS,
            "stream_url": "/stream.mp3",
        })))
    }

    pub async fn handle_get_volume(ctx: Arc<OrpheusContext>) -> Result<impl Reply, Infallible> {
        let volume = ctx.state.lock().unwrap().volume();
        Ok(warp::reply::json(&serde_json::json!({ "volume": volume })))
    }

    pub async fn handle_set_volume(
        req: VolumeRequest,
        ctx: Arc<OrpheusContext>,
    ) -> Result<impl Reply, Infallible> {
        if !req.volume.is_finite() {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": "Volume invalide" })),
                StatusCode::BAD_REQUEST,
            ));
        }

        let volume = {
            let mut s = ctx.state.lock().unwrap();
            s.set_volume(req.volume);
            s.volume()
        };

        Ok(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({ "message": "Volume mis à jour", "volume": volume }),
            ),
            StatusCode::OK,
        ))
    }

    /// Baisse le volume d'un cran.
    pub async fn handle_volume_subtract(
        ctx: Arc<OrpheusContext>,
    ) -> Result<impl Reply, Infallible> {
        Self::step_volume(ctx, -VOLUME_STEP)
    }

    /// Monte le volume d'un cran.
    pub async fn handle_volume_add(ctx: Arc<OrpheusContext>) -> Result<impl Reply, Infallible> {
        Self::step_volume(ctx, VOLUME_STEP)
    }

    /// Déplace le volume de `delta`.
    ///
    /// Le bornage est celui de `set_volume` : arrivé à 0 ou à 1, les appels
    /// suivants ne font plus rien plutôt que de renvoyer une erreur. Une touche de
    /// Stream Deck maintenue enfoncée ne doit pas se mettre à échouer.
    fn step_volume(ctx: Arc<OrpheusContext>, delta: f32) -> Result<impl Reply, Infallible> {
        let volume = {
            let mut s = ctx.state.lock().unwrap();
            let cible = s.volume() + delta;
            s.set_volume(cible);
            s.volume()
        };

        Ok(warp::reply::json(
            &serde_json::json!({ "message": "Volume mis à jour", "volume": volume }),
        ))
    }

    /// Repart sur une nouvelle graine : autre tonalité, autre progression, autre
    /// tempo. Le flux n'est pas interrompu, les auditeurs restent connectés.
    pub async fn handle_regenerate(ctx: Arc<OrpheusContext>) -> Result<impl Reply, Infallible> {
        // Graine tirée de l'horloge : pas besoin d'un générateur pour une valeur
        // unique par appel, et cela reste reproductible puisqu'elle est renvoyée.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED);

        ctx.state.lock().unwrap().request_regeneration(seed);
        log_info(format!("Régénération demandée — graine {seed}"));

        Ok(warp::reply::json(&serde_json::json!({
            "message": "Nouvelle génération en cours",
            "seed": seed,
        })))
    }
}
