//! Boucle temps réel : rendu, encodage, diffusion.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use janus_nucleus::logger::log_info;
use janus_nucleus::stream::{spawn_encode_loop, StreamHub, OUTPUT_SAMPLE_RATE};

use crate::model::OrpheusState;
use crate::render::Synth;

/// Démarre le moteur : la boucle est celle de `janus_nucleus`, seule la synthèse
/// des blocs est propre au générateur.
pub fn spawn(
    state: Arc<Mutex<OrpheusState>>,
    hub: Arc<StreamHub>,
    bitrate_kbps: u16,
    base_bpm: f32,
    seed: u64,
    lead: Duration,
    shutdown: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let mut synth = Synth::new(OUTPUT_SAMPLE_RATE as f32, base_bpm, seed);
    let mut regeneration = 0u64;

    spawn_encode_loop(hub, bitrate_kbps, lead, shutdown, "orpheus", move |pcm| {
        // Une seule prise de verrou par bloc, et rien de coûteux dedans : le fil de
        // rendu ne doit jamais attendre un handler HTTP.
        let (volume, demande, graine) = {
            let s = state.lock().unwrap();
            (s.volume(), s.regeneration(), s.pending_seed())
        };

        if demande != regeneration {
            regeneration = demande;
            synth.regenerate(graine, base_bpm);
            log_info(format!("Nouvelle génération — graine {graine}"));
        }

        synth.render(pcm, volume);

        // L'instantané est déposé après le rendu, donc toujours cohérent avec ce que
        // les auditeurs sont en train d'entendre.
        if let Ok(mut s) = state.lock() {
            s.publish_snapshot(synth.snapshot());
        }
    })
}
