//! Gain de normalisation, **sans jamais bloquer l'appelant**.
//!
//! `NormalizationManager::get_or_compute_gain` décode le fichier entier quand le
//! cache est froid, ce qui prend plusieurs secondes. L'appeler sur le chemin
//! d'exécution fige tout ce qui attend derrière : le producteur d'un flux, donc
//! tous ses auditeurs — mais aussi, pour un lecteur local, chaque requête HTTP,
//! puisque l'analyse se faisait verrou en main.
//!
//! On se contente donc du cache, et l'analyse part en tâche de fond : la piste
//! passe sans gain cette fois-ci, la suivante est prête à temps, et les écoutes
//! ultérieures sont normalisées.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use janus_library_nucleus::audio::NormalizationManager;
use janus_log_nucleus::log_error;

/// Gain à appliquer maintenant, sans analyse synchrone.
///
/// Renvoie `1.0` quand la normalisation est désactivée ou que le cache est froid ;
/// dans ce dernier cas l'analyse est lancée en tâche de fond pour les prochaines
/// écoutes.
pub fn resolve_gain(path: &Path, enabled: bool, manager: &Arc<NormalizationManager>) -> f32 {
    if !enabled {
        return 1.0;
    }

    match manager.cached_gain(path) {
        Some(gain) => gain,
        None => {
            warm_gain(manager, path.to_path_buf());
            1.0
        }
    }
}

/// Préchauffe la piste qui suit, pour que la transition, elle, soit normalisée.
///
/// Séparé de [`resolve_gain`] : la piste suivante se lit sur la playlist, que
/// l'appelant tient déjà en main au moment d'enchaîner. Les fusionner obligerait
/// à emprunter la playlist deux fois — une fois en écriture pour dépiler, une
/// fois en lecture pour regarder devant.
pub fn warm_next(manager: &Arc<NormalizationManager>, enabled: bool, next: Option<&Path>) {
    if !enabled {
        return;
    }
    let Some(next) = next else { return };
    if manager.cached_gain(next).is_none() {
        warm_gain(manager, next.to_path_buf());
    }
}

/// Calcule un gain hors du chemin d'exécution.
///
/// Le test `cached_gain().is_none()` en amont limite naturellement les doublons :
/// une fois le cache chaud, plus aucun fil n'est lancé pour ce fichier.
pub fn warm_gain(manager: &Arc<NormalizationManager>, path: PathBuf) {
    let manager = Arc::clone(manager);
    let spawned = thread::Builder::new()
        .name("janus-loudness".to_string())
        .spawn(move || {
            manager.get_or_compute_gain(&path);
        });

    if let Err(e) = spawned {
        log_error(format!("Analyse de loudness non lancée : {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desactivee_le_gain_est_neutre_et_rien_n_est_analyse() {
        let manager = Arc::new(NormalizationManager::default());
        let chemin = Path::new("n-existe-pas.mp3");

        assert_eq!(resolve_gain(chemin, false, &manager), 1.0);
        assert!(manager.cached_gain(chemin).is_none());
    }

    /// Le point qui compte : cache froid = gain neutre **immédiat**, jamais une
    /// analyse synchrone.
    #[test]
    fn un_cache_froid_rend_un_gain_neutre_sans_bloquer() {
        let manager = Arc::new(NormalizationManager::default());
        let chemin = Path::new("n-existe-pas.mp3");

        let debut = std::time::Instant::now();
        assert_eq!(resolve_gain(chemin, true, &manager), 1.0);
        assert!(
            debut.elapsed() < std::time::Duration::from_millis(200),
            "resolve_gain a bloqué"
        );
    }

    #[test]
    fn warm_next_ne_fait_rien_sans_piste_suivante() {
        let manager = Arc::new(NormalizationManager::default());
        warm_next(&manager, true, None);
        warm_next(&manager, false, Some(Path::new("x.mp3")));
    }
}
