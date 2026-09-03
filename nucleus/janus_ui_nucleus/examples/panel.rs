//! Démonstration de la brique : `cargo run -p janus_ui_nucleus --example panel`.
//!
//! Montre les trois choses qu'un enfant fait avec ce crate : régler une valeur,
//! lancer un travail de fond qu'on peut arrêter, et voir sa panne s'afficher.

use std::time::Duration;

use janus_ui_nucleus::{egui, ControlPanel, TaskHandle};

struct Demo {
    volume: f32,
    tache: Option<TaskHandle>,
    statut: String,
}

impl ControlPanel for Demo {
    fn title(&self) -> String {
        "janus_ui_nucleus — démonstration".to_string()
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        // Une panne du fil doit s'afficher : c'est tout l'intérêt de `take_error`.
        if let Some(handle) = &self.tache {
            if let Some(erreur) = handle.take_error() {
                self.statut = format!("Interrompu : {erreur}");
                self.tache = None;
            }
        }

        ui.heading("Panneau de contrôle");
        ui.separator();

        ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).text("Volume"));
        ui.add_space(10.0);

        match &self.tache {
            None => {
                if ui.button("Démarrer le travail de fond").clicked() {
                    let handle = TaskHandle::new();
                    let fil = handle.clone();
                    std::thread::spawn(move || {
                        let debut = std::time::Instant::now();
                        while !fil.should_stop() {
                            if debut.elapsed() > Duration::from_secs(5) {
                                fil.fail("cinq secondes, pour montrer la remontée");
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    });
                    self.tache = Some(handle);
                    self.statut = "En cours…".to_string();
                }
            }
            Some(handle) => {
                if ui.button("Arrêter").clicked() {
                    handle.stop();
                    self.tache = None;
                    self.statut = "Arrêt demandé.".to_string();
                }
            }
        }

        ui.separator();
        ui.label(format!("Statut : {}", self.statut));
    }

    fn on_close(&mut self) {
        if let Some(handle) = &self.tache {
            handle.stop();
        }
    }
}

fn main() -> janus_ui_nucleus::RunResult {
    janus_ui_nucleus::run(Demo {
        volume: 0.5,
        tache: None,
        statut: "Au repos.".to_string(),
    })
}
