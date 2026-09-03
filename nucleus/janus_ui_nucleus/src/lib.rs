//! Capacité « interface graphique » : un panneau de contrôle natif.
//!
//! À distinguer de [`janus_logwindow_nucleus`], qui affiche du texte et rien
//! d'autre. Ici l'enfant expose des réglages : curseurs, listes, boutons.
//!
//! Le modèle vient de `line`, la seule IHM éprouvée de l'écosystème, dont trois
//! décisions sont reprises telles quelles parce qu'elles corrigeaient des défauts
//! réels :
//!
//! - un fil de travail qu'on arrête par un drapeau atomique, jamais en tuant le
//!   fil ;
//! - **l'erreur du fil remonte à l'écran** — sans ça, une panne terminait le fil
//!   sur un `eprintln!` invisible en mode fenêtré, et l'interface continuait
//!   d'afficher « en cours… » alors que plus rien ne tournait ;
//! - la fermeture de la fenêtre vaut demande d'arrêt, ce qui donne au serveur un
//!   point de sortie propre.

pub use eframe::egui;
/// Réexporté pour l'enfant qui a besoin des options natives (taille de fenêtre,
/// icône). Le cas courant n'en a pas besoin : [`run`] et [`RunResult`] suffisent.
pub use eframe;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Ce que rend [`run`].
///
/// Aliasé pour qu'un enfant puisse écrire `fn main() -> RunResult` sans déclarer
/// eframe dans son `Cargo.toml` : la brique masque sa propre dépendance, sinon
/// « prendre la capacité interface graphique » en imposerait deux au lieu d'une.
pub type RunResult = eframe::Result<()>;

/// Ce qu'un enfant fournit pour obtenir une fenêtre de contrôle.
pub trait ControlPanel {
    /// Titre de la fenêtre.
    fn title(&self) -> String;

    /// Dessine le contenu du panneau. Appelé à chaque trame.
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Cadence de rafraîchissement.
    ///
    /// Un panneau ne montrant que des réglages n'a pas besoin de se redessiner
    /// vite ; un panneau montrant un niveau, si. 100 ms par défaut, comme `line`.
    fn repaint_after(&self) -> std::time::Duration {
        std::time::Duration::from_millis(100)
    }

    /// Appelé une fois quand la fenêtre se ferme.
    ///
    /// C'est là qu'un enfant lève son drapeau d'arrêt : fermer la fenêtre doit
    /// arrêter le serveur, pas le laisser tourner sans pilote.
    fn on_close(&mut self) {}
}

/// Ouvre la fenêtre et rend la main quand elle se ferme.
pub fn run<P: ControlPanel + 'static>(panel: P) -> RunResult {
    let title = panel.title();
    eframe::run_native(
        &title,
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(Host { panel, closed: false }))),
    )
}

struct Host<P: ControlPanel> {
    panel: P,
    closed: bool,
}

impl<P: ControlPanel> eframe::App for Host<P> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| self.panel.ui(ui));

        if ctx.input(|i| i.viewport().close_requested()) && !self.closed {
            self.closed = true;
            self.panel.on_close();
        }

        ctx.request_repaint_after(self.panel.repaint_after());
    }
}

/// Un fil de travail qu'on arrête proprement, et dont l'erreur remonte à l'écran.
///
/// Le drapeau et la boîte à erreur sont réunis ici parce qu'ils vont toujours
/// ensemble : un fil qu'on peut arrêter mais dont l'échec reste invisible laisse
/// l'interface mentir sur son état.
#[derive(Clone, Default)]
pub struct TaskHandle {
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

impl TaskHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Le fil doit-il s'arrêter ? À interroger régulièrement depuis le travail.
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Demande l'arrêt. Le fil s'arrête à sa prochaine vérification.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Déclare l'échec du travail. À appeler depuis le fil.
    pub fn fail(&self, message: impl Into<String>) {
        if let Ok(mut slot) = self.error.lock() {
            *slot = Some(message.into());
        }
    }

    /// Récupère l'erreur s'il y en a une, et vide la boîte.
    ///
    /// À appeler depuis [`ControlPanel::ui`] : c'est ce qui fait qu'une panne
    /// s'affiche au lieu de passer inaperçue.
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().ok()?.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_arret_se_propage_aux_clones() {
        let handle = TaskHandle::new();
        let vu_par_le_fil = handle.clone();

        assert!(!vu_par_le_fil.should_stop());
        handle.stop();
        assert!(vu_par_le_fil.should_stop());
    }

    /// La propriété qui compte : une panne survenue dans le fil doit être
    /// récupérable par l'interface, sinon elle reste invisible en mode fenêtré.
    #[test]
    fn l_erreur_du_fil_remonte_a_l_interface() {
        let handle = TaskHandle::new();
        let vu_par_le_fil = handle.clone();

        assert!(handle.take_error().is_none());
        vu_par_le_fil.fail("périphérique disparu");
        assert_eq!(handle.take_error().as_deref(), Some("périphérique disparu"));
    }

    #[test]
    fn l_erreur_ne_se_lit_qu_une_fois() {
        let handle = TaskHandle::new();
        handle.fail("panne");
        assert!(handle.take_error().is_some());
        assert!(handle.take_error().is_none(), "l'erreur a été relue");
    }
}
