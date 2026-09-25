use rodio::Sink;
use std::sync::{Arc, Mutex, mpsc};

use janus_library_nucleus::audio::NormalizationManager;
use janus_config_nucleus::update_config_key;
use janus_log_nucleus::{log_error, log_info};

/// Un son en cours : son sink, et le canal qui arrête le thread qui le surveille.
///
/// Les deux vont ensemble et partent ensemble. Ils vivaient dans deux listes
/// séparées, et le canal d'un son terminé naturellement n'était jamais retiré : la
/// liste grossissait jusqu'au prochain `/stop`.
pub struct ActiveSound {
    pub sink: Arc<Mutex<Sink>>,
    pub stop: mpsc::Sender<()>,
}

/// Pause de la musique tenue par le soundboard.
///
/// Plusieurs sons peuvent se chevaucher. Seul le premier d'une série interroge
/// JanusCore et le met en pause ; la musique ne reprend qu'à la fin du **dernier**,
/// et seulement si c'est le soundboard qui l'avait arrêtée. Chaque son décidait
/// auparavant pour lui-même : le second trouvait la musique en pause, retenait
/// « elle ne jouait pas », et la fin du premier la relançait sous le second.
#[derive(Default, Debug)]
pub struct MusicHold {
    active: usize,
    resume: bool,
}

impl MusicHold {
    /// Un son commence. Vrai pour le premier d'une série : c'est à lui de décider
    /// de la pause.
    pub fn begin(&mut self) -> bool {
        self.active += 1;
        self.active == 1
    }

    /// Retient que la musique jouait et que le soundboard l'a mise en pause.
    pub fn set_resume(&mut self, resume: bool) {
        self.resume = resume;
    }

    /// Un son se termine, quelle qu'en soit la cause. Vrai quand c'était le dernier
    /// et que la musique doit reprendre.
    pub fn end(&mut self) -> bool {
        self.active = self.active.saturating_sub(1);
        if self.active == 0 && self.resume {
            self.resume = false;
            return true;
        }
        false
    }
}

pub struct PlayerState {
    pub stream_handle: rodio::OutputStreamHandle,
    pub volume: f32,
    /// Sons du soundboard en cours de lecture.
    pub sounds: Vec<ActiveSound>,
    /// Pause de la musique décidée par le soundboard, partagée par les sons en cours.
    pub hold: MusicHold,
    // Gestionnaire de normalisation partagé
    pub normalization_manager: Arc<NormalizationManager>,
}

impl PlayerState {
    pub fn new(stream_handle: rodio::OutputStreamHandle, initial_volume: f32) -> Self {
        Self {
            stream_handle,
            volume: initial_volume,
            sounds: Vec::new(),
            hold: MusicHold::default(),
            normalization_manager: Arc::new(NormalizationManager::default()),
        }
    }

    #[allow(dead_code)]
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        // Mettre à jour le volume de tous les sinks actifs de la soundboard
        for sound in &self.sounds {
            if let Ok(s) = sound.sink.lock() {
                // On ne peut pas facilement réappliquer la normalisation ici sans stocker le gain par sink
                // Pour l'instant on applique juste le volume global
                // Idéalement, le sink devrait connaître son gain de base.
                s.set_volume(volume);
            }
        }
        // Mettre à jour uniquement la clé VOLUME dans env.json
        if let Err(e) = update_config_key("VOLUME", serde_json::json!(volume)) {
            log_error(format!(
                "Erreur lors de la mise à jour du volume dans env.json: {}",
                e
            ));
        }
    }

    pub fn add_sound(&mut self, sound: ActiveSound) {
        self.sounds.push(sound);
        log_info(format!(
            "Son ajouté au soundboard. Sons en cours : {}",
            self.sounds.len()
        ));
    }

    /// Signale l'arrêt à tous les sons en cours et vide la liste.
    ///
    /// N'attend pas que les threads s'arrêtent : chacun se retire seul et passe par
    /// la reprise de la musique. L'ancienne version dormait 200 ms en tenant le verrou
    /// du player, depuis un handler async.
    pub fn stop_all_sounds(&mut self) {
        log_info(format!("Arrêt de {} son(s) du soundboard", self.sounds.len()));
        for sound in self.sounds.drain(..) {
            // Échoue seulement si le thread est déjà terminé : rien à arrêter.
            let _ = sound.stop.send(());
        }
    }

    pub fn remove_sound(&mut self, sink: &Arc<Mutex<Sink>>) {
        self.sounds.retain(|sound| !Arc::ptr_eq(&sound.sink, sink));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deux_sons_superposes_ne_reprennent_la_musique_qu_une_fois_a_la_fin_du_second() {
        let mut hold = MusicHold::default();

        assert!(hold.begin(), "le premier son décide de la pause");
        hold.set_resume(true);
        assert!(!hold.begin(), "le second trouve la musique déjà arrêtée par nous");

        assert!(!hold.end(), "la fin du premier ne doit pas relancer sous le second");
        assert!(hold.end(), "la fin du second relance la musique");
    }

    #[test]
    fn une_musique_deja_en_pause_ne_reprend_jamais() {
        // Pause manuelle : le premier son voit une musique arrêtée et ne retient rien.
        let mut hold = MusicHold::default();
        assert!(hold.begin());
        assert!(!hold.end());
    }

    #[test]
    fn la_reprise_ne_sert_qu_une_fois() {
        let mut hold = MusicHold::default();
        hold.begin();
        hold.set_resume(true);
        assert!(hold.end());

        // Série suivante, musique mise en pause à la main entre-temps.
        assert!(hold.begin());
        assert!(!hold.end());
    }

    #[test]
    fn une_fin_en_trop_ne_fait_pas_deborder_le_compteur() {
        let mut hold = MusicHold::default();
        assert!(!hold.end());
        assert!(hold.begin(), "le compteur est resté à zéro");
    }
}
