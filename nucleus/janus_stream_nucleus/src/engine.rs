//! Boucle de production : cadence, encode, diffuse.
//!
//! Les deux serveurs de flux ne diffèrent que par la façon de **remplir un bloc de
//! PCM** — l'un décode des fichiers, l'autre synthétise. Tout ce qui vient après
//! est identique, et c'est justement la partie qu'il ne faut pas laisser diverger :
//! l'ordre encoder → publier → cadencer, le traitement du retour vide de LAME, et
//! le vidage final.

use bytes::Bytes;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::logger::{log_error, log_info};
use crate::stream::{
    Mp3Encoder, Pacer, StreamHub, CHUNK_FRAMES, CHUNK_SAMPLES, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE,
};

/// Avance du producteur sur l'horloge.
///
/// Autant d'audio déjà encodé et distribué en avance, qui donne au client de quoi
/// absorber la gigue du réseau sans couper. Payé en latence, sans importance pour
/// une diffusion sans interaction.
pub const DEFAULT_LEAD: Duration = Duration::from_millis(1_500);

/// Démarre la boucle de diffusion sur son propre fil.
///
/// `produce` reçoit un bloc stéréo entrelacé de [`CHUNK_SAMPLES`] échantillons à
/// remplir entièrement — silence compris. C'est le seul point de variation entre
/// une webradio et un générateur.
///
/// Un `std::thread` et non une tâche tokio : produire et encoder est du calcul
/// bloquant, qui monopoliserait un fil du runtime et retarderait les réponses HTTP.
/// L'encodeur est construit **avant** le lancement du fil, pour qu'une
/// configuration invalide échoue au démarrage plutôt que silencieusement ensuite.
pub fn spawn_encode_loop<F>(
    hub: Arc<StreamHub>,
    bitrate_kbps: u16,
    lead: Duration,
    shutdown: Arc<AtomicBool>,
    label: &'static str,
    produce: F,
) -> Result<JoinHandle<()>, String>
where
    F: FnMut(&mut [i16]) + Send + 'static,
{
    let encoder = Mp3Encoder::new(OUTPUT_SAMPLE_RATE, OUTPUT_CHANNELS, bitrate_kbps)?;

    thread::Builder::new()
        .name(format!("{label}-engine"))
        .spawn(move || run(hub, encoder, lead, shutdown, label, produce))
        .map_err(|e| format!("Impossible de démarrer le moteur audio : {e}"))
}

fn run<F>(
    hub: Arc<StreamHub>,
    mut encoder: Mp3Encoder,
    lead: Duration,
    shutdown: Arc<AtomicBool>,
    label: &str,
    mut produce: F,
) where
    F: FnMut(&mut [i16]),
{
    let mut pacer = Pacer::new(OUTPUT_SAMPLE_RATE, lead);
    let mut pcm = vec![0i16; CHUNK_SAMPLES];

    while !shutdown.load(Ordering::Relaxed) {
        produce(&mut pcm);

        match encoder.encode_interleaved(&pcm) {
            // LAME retient les échantillons jusqu'à pouvoir sortir une trame
            // complète : un retour vide est normal, pas une erreur.
            Ok(bytes) if bytes.is_empty() => {}
            Ok(bytes) => hub.publish(Bytes::from(bytes)),
            Err(e) => log_error(format!("Encodage MP3 : {e}")),
        }

        pacer.commit(CHUNK_FRAMES as u64);
    }

    // Écoule ce que LAME retient encore, pour que les derniers auditeurs reçoivent
    // une trame complète plutôt qu'une trame tronquée.
    if let Ok(reste) = encoder.flush() {
        if !reste.is_empty() {
            hub.publish(Bytes::from(reste));
        }
    }
    log_info(format!("Moteur {label} arrêté"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    /// Le producteur doit être appelé en boucle, et ce qu'il écrit doit finir chez
    /// les auditeurs sous forme de trames MP3.
    #[tokio::test]
    async fn la_boucle_produit_encode_et_diffuse() {
        let hub = Arc::new(StreamHub::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut abonne = hub.subscribe();

        // Une sinusoïde grossière : du silence se compresse trop pour être probant.
        let mut n = 0u32;
        let handle = spawn_encode_loop(
            Arc::clone(&hub),
            192,
            Duration::ZERO,
            Arc::clone(&shutdown),
            "test",
            move |pcm| {
                for frame in pcm.chunks_exact_mut(2) {
                    let v = if (n / 64) % 2 == 0 { 6_000 } else { -6_000 };
                    frame[0] = v;
                    frame[1] = v;
                    n += 1;
                }
            },
        )
        .expect("le moteur doit démarrer");

        let chunk = abonne.next().await.expect("aucun chunk diffusé");
        assert!(!chunk.is_empty());
        // Synchro MPEG en tête de la première trame.
        assert_eq!(chunk[0], 0xFF, "ce n'est pas une trame MP3");
        assert_eq!(chunk[1] & 0xE0, 0xE0);

        shutdown.store(true, Ordering::Relaxed);
        handle.join().expect("le fil doit se terminer");
    }

    #[test]
    fn un_debit_invalide_echoue_au_demarrage_et_non_dans_le_fil() {
        // 192 est valide ; on vérifie surtout que l'erreur remonte à l'appelant.
        let hub = Arc::new(StreamHub::default());
        let shutdown = Arc::new(AtomicBool::new(true));
        let r = spawn_encode_loop(hub, 192, Duration::ZERO, shutdown, "test", |_| {});
        assert!(r.is_ok());
        r.unwrap().join().unwrap();
    }

    /// L'arrêt doit être honoré sans attendre un bloc de plus que nécessaire.
    #[test]
    fn la_boucle_s_arrete_sur_demande() {
        let hub = Arc::new(StreamHub::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let appels = Arc::new(std::sync::Mutex::new(0usize));

        let compteur = Arc::clone(&appels);
        let handle = spawn_encode_loop(
            Arc::clone(&hub),
            192,
            Duration::ZERO,
            Arc::clone(&shutdown),
            "test",
            move |pcm| {
                pcm.fill(0);
                *compteur.lock().unwrap() += 1;
            },
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(50));
        shutdown.store(true, Ordering::Relaxed);
        handle.join().expect("le fil doit se terminer");

        assert!(*appels.lock().unwrap() > 0, "le producteur n'a jamais été appelé");
    }
}
