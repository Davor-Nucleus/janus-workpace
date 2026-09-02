//! Horloge musicale et décisions d'arrangement.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::theory::{build_chord, note_name, Chord, Degree, PROGRESSIONS, ROOTS};
use crate::synth::drums::Drum;

/// Grille en doubles-croches, mesure à quatre temps.
pub const STEPS_PER_BAR: u64 = 16;
/// Longueur d'une progression, en mesures.
pub const BARS_PER_PROGRESSION: u64 = 4;
/// Longueur d'une section, au bout de laquelle l'énergie peut changer.
pub const BARS_PER_SECTION: u64 = 8;

/// Ce que l'arrangeur demande au moteur de jouer sur un pas.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Nouvel accord pour la nappe.
    PadChord(Vec<i32>),
    /// La nappe se tait (l'énergie est retombée au plus bas).
    PadOff,
    Bass(i32),
    Arp(i32),
    Lead(i32),
    Percussion(Drum),
}

/// Instantané lisible de l'état musical, pour l'API.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub bpm: f32,
    pub key: String,
    pub chord: String,
    pub bar: u64,
    pub section: u64,
    pub energy: u8,
    pub seed: u64,
}

pub struct Arranger {
    rng: StdRng,
    sample_rate: f32,
    seed: u64,

    bpm: f32,
    root: i32,
    progression: [Degree; 4],

    /// Position dans la grille, en échantillons puis en pas.
    samples_per_step: f32,
    step_accum: f32,
    step: u64,
    /// Faux tant qu'aucun pas n'a encore été joué : le tout premier doit tomber
    /// immédiatement, sinon la musique démarre avec un temps de retard.
    started: bool,

    energy: u8,
    /// Mélodie de la section courante, en degrés relatifs à la tonique.
    melody: Vec<i32>,
    melody_index: usize,
    /// Index de l'accord dont la nappe joue actuellement.
    current_chord_index: Option<usize>,
}

impl Arranger {
    pub fn new(sample_rate: f32, bpm: f32, seed: u64) -> Self {
        let mut a = Self {
            rng: StdRng::seed_from_u64(seed),
            sample_rate,
            seed,
            bpm,
            root: ROOTS[0],
            progression: PROGRESSIONS[0],
            samples_per_step: 1.0,
            step_accum: 0.0,
            step: 0,
            started: false,
            energy: 2,
            melody: Vec::new(),
            melody_index: 0,
            current_chord_index: None,
        };
        a.regenerate(seed, bpm);
        a
    }

    /// Repart d'une nouvelle graine : nouvelle tonalité, nouvelle progression,
    /// tempo légèrement retiré au sort autour de la consigne.
    pub fn regenerate(&mut self, seed: u64, base_bpm: f32) {
        self.rng = StdRng::seed_from_u64(seed);
        self.seed = seed;

        self.root = ROOTS[self.rng.gen_range(0..ROOTS.len())];
        self.progression = PROGRESSIONS[self.rng.gen_range(0..PROGRESSIONS.len())];
        // ±6 BPM autour de la consigne : de quoi que deux sessions ne se
        // ressemblent pas, sans sortir du tempo du genre.
        self.bpm = (base_bpm + self.rng.gen_range(-6.0..6.0)).clamp(70.0, 120.0);

        // Une noire vaut quatre pas de la grille.
        self.samples_per_step = 60.0 / self.bpm / 4.0 * self.sample_rate;
        self.step_accum = 0.0;
        self.step = 0;
        self.started = false;
        self.energy = 2;
        self.current_chord_index = None;
        self.regenerate_melody();
    }

    fn regenerate_melody(&mut self) {
        // Huit notes par section, contraintes à des sauts d'au plus une quinte :
        // un tirage totalement libre s'entend comme du hasard, pas comme une mélodie.
        let mut melody = Vec::with_capacity(8);
        let mut degree = self.rng.gen_range(0..4);
        for _ in 0..8 {
            let saut = self.rng.gen_range(-3i32..=3);
            degree = (degree + saut).clamp(-2, 9);
            melody.push(degree);
        }
        self.melody = melody;
        self.melody_index = 0;
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    fn bar(&self) -> u64 {
        self.step / STEPS_PER_BAR
    }

    fn chord_index(&self) -> usize {
        (self.bar() % BARS_PER_PROGRESSION) as usize
    }

    fn current_chord(&self) -> Chord {
        build_chord(self.root, self.progression[self.chord_index()], true)
    }

    /// Ouverture globale du filtre, pilotée par l'énergie.
    ///
    /// C'est l'effet le plus audible de la progression d'énergie : le morceau
    /// s'éclaircit à mesure que les instruments entrent.
    pub fn cutoff_scale(&self) -> f32 {
        match self.energy {
            0 => 0.45,
            1 => 0.7,
            2 => 1.0,
            _ => 1.4,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let chord = self.current_chord();
        Snapshot {
            bpm: self.bpm,
            key: format!("{} mineur", note_name(self.root)),
            chord: format!("{} ({})", chord.degree.label(), note_name(chord.root())),
            bar: self.bar(),
            section: self.bar() / BARS_PER_SECTION,
            energy: self.energy,
            seed: self.seed,
        }
    }

    /// Avance d'un échantillon ; renvoie les événements si un pas vient d'être franchi.
    pub fn advance(&mut self) -> Option<Vec<Event>> {
        if !self.started {
            self.started = true;
            return Some(self.events_for_step());
        }

        self.step_accum += 1.0;
        if self.step_accum < self.samples_per_step {
            return None;
        }
        // Soustraction plutôt que remise à zéro : `samples_per_step` n'est pas
        // entier, et repartir de zéro accumulerait un retard de tempo audible.
        self.step_accum -= self.samples_per_step;
        self.step += 1;

        // Frontière de section : l'énergie évolue.
        if self.step % (STEPS_PER_BAR * BARS_PER_SECTION) == 0 {
            self.step_energy();
            self.regenerate_melody();
        }

        Some(self.events_for_step())
    }

    /// Marche aléatoire de l'énergie, **avec rappel vers le haut**.
    ///
    /// Une marche symétrique simplement bornée serait un piège : le bornage rend
    /// les extrémités collantes, la distribution devient uniforme, et l'énergie 0
    /// — la nappe seule, sans batterie ni basse — occupe un quart du temps par
    /// séries pouvant dépasser vingt sections. Mesuré sur sept minutes de flux,
    /// cela donnait des minutes entières à −20 dB : la radio s'entend décrocher.
    ///
    /// Le rappel garde l'énergie 0 comme une respiration d'une seule section, et
    /// concentre le reste du temps sur 2 et 3.
    fn step_energy(&mut self) {
        let roll = self.rng.gen_range(0..10);
        let delta: i32 = match self.energy {
            // On ne s'installe jamais dans le creux : une section, puis on remonte.
            0 => 1,
            // Au plafond, on redescend plus souvent qu'on ne s'y maintient.
            3 => {
                if roll < 6 {
                    -1
                } else {
                    0
                }
            }
            _ => match roll {
                0..=4 => 1,
                5..=7 => -1,
                _ => 0,
            },
        };
        self.energy = (self.energy as i32 + delta).clamp(0, 3) as u8;
    }

    fn events_for_step(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let step_in_bar = self.step % STEPS_PER_BAR;
        let bar_in_section = self.bar() % BARS_PER_SECTION;
        let is_fill_bar = bar_in_section == BARS_PER_SECTION - 1;
        let chord = self.current_chord();

        // --- Nappe : uniquement au changement d'accord ---
        if step_in_bar == 0 {
            let index = self.chord_index();
            if self.energy == 0 && self.current_chord_index.is_some() {
                events.push(Event::PadOff);
                self.current_chord_index = None;
            } else if self.energy > 0 && self.current_chord_index != Some(index) {
                events.push(Event::PadChord(chord.tones.clone()));
                self.current_chord_index = Some(index);
            }
        }

        // --- Basse ---
        if self.energy >= 1 {
            let joue = if self.energy >= 2 {
                true // doubles-croches continues
            } else {
                step_in_bar % 4 == 0 // sur les temps
            };
            if joue {
                // Saut d'octave sur les temps faibles : le moteur rythmique du genre.
                let octave = if self.energy >= 2 && step_in_bar % 4 == 2 { 12 } else { 0 };
                events.push(Event::Bass(chord.root() - 12 + octave));
            }
        }

        // --- Arpège ---
        if self.energy >= 2 {
            let index = (self.step % 8) as usize;
            events.push(Event::Arp(chord.tone_at(index) + 12));
        }

        // --- Lead ---
        if self.energy >= 3 && matches!(step_in_bar, 0 | 6 | 10) {
            let degre = self.melody[self.melody_index % self.melody.len()];
            self.melody_index += 1;
            events.push(Event::Lead(super::theory::scale_note(self.root, degre) + 24));
        }

        // --- Batterie ---
        match self.energy {
            0 => {}
            1 => {
                if step_in_bar == 0 || step_in_bar == 8 {
                    events.push(Event::Percussion(Drum::Kick));
                }
            }
            _ => {
                if step_in_bar % 4 == 0 {
                    events.push(Event::Percussion(Drum::Kick));
                }
                if step_in_bar == 4 || step_in_bar == 12 {
                    events.push(Event::Percussion(Drum::Snare));
                }
                let hat = if self.energy >= 3 { 1 } else { 2 };
                if step_in_bar % hat == 0 {
                    events.push(Event::Percussion(Drum::HatClosed));
                }
                if step_in_bar == 14 {
                    events.push(Event::Percussion(Drum::HatOpen));
                }
                // Roulement sur la dernière mesure de section : c'est ce qui annonce
                // le changement et évite que la boucle paraisse mécanique.
                if is_fill_bar && step_in_bar >= 12 {
                    events.push(Event::Percussion(Drum::Snare));
                }
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::theory::in_scale;

    const FS: f32 = 48_000.0;

    /// Déroule exactement `bars` mesures et renvoie les événements, avec leur pas.
    ///
    /// On compte les pas et non les échantillons : borner en échantillons ferait
    /// déborder d'un pas sur la mesure suivante, ce qui décalerait tous les
    /// comptages d'un événement.
    fn derouler(a: &mut Arranger, bars: u64) -> Vec<(u64, Event)> {
        let total_steps = bars * STEPS_PER_BAR;
        let mut sortie = Vec::new();
        let mut pas = 0u64;
        while pas < total_steps {
            if let Some(events) = a.advance() {
                for e in events {
                    sortie.push((pas, e));
                }
                pas += 1;
            }
        }
        sortie
    }

    #[test]
    fn le_premier_pas_tombe_immediatement() {
        let mut a = Arranger::new(FS, 92.0, 1);
        assert!(a.advance().is_some(), "la musique démarre avec un temps de retard");
    }

    #[test]
    fn la_grille_suit_le_tempo() {
        let mut a = Arranger::new(FS, 92.0, 1);
        let bpm = a.bpm();
        // Une mesure à 4/4 dure 4 noires.
        let attendu = 60.0 / bpm * 4.0 * FS;

        let mut pas = 0;
        let mut echantillons = 0usize;
        while pas <= STEPS_PER_BAR {
            if a.advance().is_some() {
                pas += 1;
            }
            echantillons += 1;
        }
        let ecart = (echantillons as f32 - attendu).abs();
        assert!(ecart < FS * 0.01, "dérive de {ecart} échantillons sur une mesure");
    }

    /// Le point à ne pas rater : `samples_per_step` n'est pas entier, donc une
    /// remise à zéro de l'accumulateur accumulerait un retard. Sur mille mesures
    /// l'écart doit rester négligeable.
    #[test]
    fn le_tempo_ne_derive_pas_sur_la_duree() {
        let mut a = Arranger::new(FS, 92.0, 3);
        let bpm = a.bpm();
        let bars = 200u64;
        let attendu = 60.0 / bpm * 4.0 * bars as f32 * FS;

        let mut pas = 0u64;
        let mut echantillons = 0usize;
        while pas < bars * STEPS_PER_BAR {
            if a.advance().is_some() {
                pas += 1;
            }
            echantillons += 1;
        }
        let ecart_relatif = (echantillons as f32 - attendu).abs() / attendu;
        assert!(ecart_relatif < 0.001, "dérive relative de {ecart_relatif}");
    }

    #[test]
    fn aucune_note_emise_ne_sort_de_la_gamme() {
        for seed in [1u64, 7, 42, 1234, 99999] {
            let mut a = Arranger::new(FS, 92.0, seed);
            let root = a.root;
            for (pas, event) in derouler(&mut a, 64) {
                let notes: Vec<i32> = match event {
                    Event::PadChord(t) => t,
                    Event::Bass(n) | Event::Arp(n) | Event::Lead(n) => vec![n],
                    _ => continue,
                };
                for n in notes {
                    assert!(
                        in_scale(root, n),
                        "graine {seed}, pas {pas} : {n} hors de la gamme de {root}"
                    );
                }
            }
        }
    }

    /// Deux arrangeurs de même graine doivent produire exactement la même suite.
    #[test]
    fn la_meme_graine_produit_le_meme_arrangement() {
        let mut a = Arranger::new(FS, 92.0, 4242);
        let mut b = Arranger::new(FS, 92.0, 4242);
        assert_eq!(a.bpm(), b.bpm());
        assert_eq!(derouler(&mut a, 40), derouler(&mut b, 40));
    }

    #[test]
    fn deux_graines_donnent_des_arrangements_differents() {
        let mut a = Arranger::new(FS, 92.0, 1);
        let mut b = Arranger::new(FS, 92.0, 2);
        assert_ne!(derouler(&mut a, 16), derouler(&mut b, 16));
    }

    #[test]
    fn la_nappe_change_a_chaque_mesure_pas_a_chaque_pas() {
        let mut a = Arranger::new(FS, 92.0, 5);
        let accords: Vec<u64> = derouler(&mut a, 8)
            .into_iter()
            .filter(|(_, e)| matches!(e, Event::PadChord(_)))
            .map(|(pas, _)| pas)
            .collect();

        assert_eq!(accords.len(), 8, "un accord par mesure attendu");
        for p in &accords {
            assert_eq!(p % STEPS_PER_BAR, 0, "accord déclenché hors du début de mesure");
        }
    }

    #[test]
    fn la_grosse_caisse_marque_les_temps() {
        let mut a = Arranger::new(FS, 92.0, 6);
        a.energy = 2;
        let kicks: Vec<u64> = derouler(&mut a, 4)
            .into_iter()
            .filter(|(_, e)| matches!(e, Event::Percussion(Drum::Kick)))
            .map(|(pas, _)| pas % STEPS_PER_BAR)
            .collect();

        assert!(!kicks.is_empty());
        for k in kicks {
            assert_eq!(k % 4, 0, "grosse caisse hors du temps, au pas {k}");
        }
    }

    #[test]
    fn au_repos_complet_seule_la_nappe_se_tait() {
        let mut a = Arranger::new(FS, 92.0, 8);
        a.energy = 0;
        let events = derouler(&mut a, 4);
        assert!(
            !events.iter().any(|(_, e)| matches!(
                e,
                Event::Percussion(_) | Event::Bass(_) | Event::Arp(_) | Event::Lead(_)
            )),
            "de l'instrumentation joue alors que l'énergie est nulle"
        );
    }

    #[test]
    fn l_energie_reste_dans_ses_bornes() {
        let mut a = Arranger::new(FS, 92.0, 11);
        for _ in 0..2_000 {
            a.step_energy();
            assert!(a.energy <= 3, "énergie hors bornes : {}", a.energy);
        }
    }

    /// Le creux ne doit jamais s'installer.
    ///
    /// Une marche symétrique bornée passait un quart du temps à l'énergie 0 — la
    /// nappe seule — par séries de plusieurs minutes, ce qui s'entendait comme une
    /// panne plutôt que comme une respiration.
    #[test]
    fn l_energie_ne_s_installe_pas_dans_le_creux() {
        let mut a = Arranger::new(FS, 92.0, 17);
        let mut compte = [0usize; 4];
        let mut serie = 0;
        let mut plus_longue_serie = 0;

        for _ in 0..20_000 {
            a.step_energy();
            compte[a.energy as usize] += 1;
            if a.energy == 0 {
                serie += 1;
                plus_longue_serie = plus_longue_serie.max(serie);
            } else {
                serie = 0;
            }
        }

        let part_creux = compte[0] as f32 / 20_000.0;
        assert!(part_creux < 0.15, "énergie 0 pendant {:.0} % du temps", part_creux * 100.0);
        assert_eq!(
            plus_longue_serie, 1,
            "l'énergie est restée {plus_longue_serie} sections d'affilée dans le creux"
        );
        // Et le gros du temps doit se passer là où tous les instruments jouent.
        let part_dense = (compte[2] + compte[3]) as f32 / 20_000.0;
        assert!(part_dense > 0.5, "seulement {:.0} % du temps en jeu dense", part_dense * 100.0);
    }

    #[test]
    fn l_ouverture_du_filtre_croit_avec_l_energie() {
        let mut a = Arranger::new(FS, 92.0, 12);
        let mut precedent = 0.0;
        for e in 0..=3 {
            a.energy = e;
            let scale = a.cutoff_scale();
            assert!(scale > precedent, "l'ouverture ne croît pas à l'énergie {e}");
            precedent = scale;
        }
    }

    #[test]
    fn l_instantane_est_lisible() {
        let a = Arranger::new(FS, 92.0, 13);
        let s = a.snapshot();
        assert!(s.key.contains("mineur"));
        assert!(!s.chord.is_empty());
        assert!((70.0..=120.0).contains(&s.bpm), "tempo hors du genre : {}", s.bpm);
        assert_eq!(s.seed, 13);
    }
}
