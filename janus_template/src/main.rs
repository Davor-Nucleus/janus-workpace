//! Gabarit d'enfant : un synthé piloté à l'écran.
//!
//! Ce binaire n'a aucune utilité en production. Il existe pour montrer comment on
//! compose un enfant à partir des briques du kit, et pour que la composition soit
//! vérifiée par le compilateur plutôt que décrite dans un document.
//!
//! Il prend cinq briques — journalisation, configuration, plateforme, interface
//! graphique, musique procédurale — et **aucune** des quatre autres. Il ne compile
//! donc ni LAME, ni symphonia, ni rodio, ni le gros jeu de features winapi.
//! `cargo tree -p janus_template` le confirme.
//!
//! Pour démarrer un vrai enfant : copier ce dossier, ajuster la liste de
//! dépendances dans `Cargo.toml`, et l'ajouter aux `members` du workspace.

use janus_config_nucleus::read_config;
use janus_log_nucleus::{log_info, set_gui_enabled};
use janus_synth_nucleus::render::Synth;
use janus_ui_nucleus::{egui, ControlPanel};

/// Fréquence de travail. Le gabarit ne diffuse rien : il rend en mémoire, donc
/// il choisit librement. Un enfant qui diffuserait prendrait
/// `janus_stream_nucleus::OUTPUT_SAMPLE_RATE`, sur lequel LAME est verrouillé.
const SAMPLE_RATE: f32 = 48_000.0;

struct Panneau {
    synth: Synth,
    bpm: f32,
    seed: u64,
    /// Crête du dernier rendu : la preuve visible que la chaîne produit du son.
    crete: i32,
}

impl Panneau {
    fn rendre_une_seconde(&mut self) {
        let mut bloc = vec![0i16; SAMPLE_RATE as usize * 2];
        self.synth.render(&mut bloc, 1.0);
        self.crete = bloc.iter().map(|s| s.abs() as i32).max().unwrap_or(0);
    }
}

impl ControlPanel for Panneau {
    fn title(&self) -> String {
        "janus_template — synthé piloté à l'écran".to_string()
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let instantane = self.synth.snapshot();

        ui.heading("Musique procédurale");
        ui.label("Cinq briques composées : log, config, platform, ui, synth.");
        ui.separator();

        egui::Grid::new("etat").num_columns(2).show(ui, |ui| {
            ui.label("Graine");
            ui.label(instantane.seed.to_string());
            ui.end_row();
            ui.label("Tonalité");
            ui.label(&instantane.key);
            ui.end_row();
            ui.label("Accord");
            ui.label(&instantane.chord);
            ui.end_row();
            ui.label("Tempo");
            ui.label(format!("{:.0} BPM", instantane.bpm));
            ui.end_row();
            ui.label("Mesure");
            ui.label(instantane.bar.to_string());
            ui.end_row();
            ui.label("Énergie");
            ui.label(instantane.energy.to_string());
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.add(egui::Slider::new(&mut self.bpm, 60.0..=140.0).text("BPM de consigne"));

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Rendre une seconde").clicked() {
                self.rendre_une_seconde();
            }
            if ui.button("Régénérer").clicked() {
                // La graine détermine entièrement la musique : la fixer rejoue la
                // même session à l'identique.
                self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                self.synth.regenerate(self.seed, self.bpm);
                self.crete = 0;
                log_info(format!("Régénération sur la graine {}", self.seed));
            }
        });

        ui.separator();
        ui.label(if self.crete == 0 {
            "Aucun rendu depuis la dernière régénération.".to_string()
        } else {
            format!("Crête du dernier rendu : {} / {}", self.crete, i16::MAX)
        });
    }
}

fn main() -> janus_ui_nucleus::RunResult {
    janus_platform_nucleus::console::set_title("janus_template");

    // Le gabarit lit le même `env.json` que les autres enfants — c'est le seul
    // point de configuration du kit. Une valeur absente prend son repli.
    let config = read_config();
    let bpm = config.orpheus_bpm.unwrap_or(92) as f32;

    // Pas de fenêtre de logs ici : cet enfant ne prend pas la brique, donc les
    // logs partent en console uniquement.
    set_gui_enabled(false);
    log_info("Gabarit démarré".to_string());

    let seed = 0x5EED;
    janus_ui_nucleus::run(Panneau {
        synth: Synth::new(SAMPLE_RATE, bpm, seed),
        bpm,
        seed,
        crete: 0,
    })
}
