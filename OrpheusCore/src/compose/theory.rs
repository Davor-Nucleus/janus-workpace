//! Gammes, degrés et accords.
//!
//! Les degrés sont un `enum`, jamais des entiers nus : c'est ce qui garantit
//! qu'aucune note hors gamme ne peut être émise, quelle que soit la suite de
//! tirages aléatoires en amont.

/// Intervalles de la gamme mineure naturelle, en demi-tons.
pub const MINOR_SCALE: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];

/// Degré de la gamme.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Degree {
    I,
    II,
    III,
    IV,
    V,
    VI,
    VII,
}

impl Degree {
    pub const ALL: [Degree; 7] = [
        Degree::I,
        Degree::II,
        Degree::III,
        Degree::IV,
        Degree::V,
        Degree::VI,
        Degree::VII,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|d| *d == self).unwrap_or(0)
    }

    /// Chiffrage romain en mineur : minuscule pour les accords mineurs.
    pub fn label(self) -> &'static str {
        match self {
            Degree::I => "i",
            Degree::II => "ii°",
            Degree::III => "III",
            Degree::IV => "iv",
            Degree::V => "v",
            Degree::VI => "VI",
            Degree::VII => "VII",
        }
    }
}

/// Progressions caractéristiques du genre, en mineur, sur quatre mesures.
pub const PROGRESSIONS: [[Degree; 4]; 5] = [
    [Degree::I, Degree::VI, Degree::III, Degree::VII],
    [Degree::I, Degree::VII, Degree::VI, Degree::VII],
    [Degree::I, Degree::IV, Degree::VI, Degree::V],
    [Degree::VI, Degree::VII, Degree::I, Degree::I],
    [Degree::I, Degree::III, Degree::VII, Degree::VI],
];

/// Toniques possibles, notes MIDI dans le grave.
pub const ROOTS: [i32; 5] = [45, 43, 41, 40, 48]; // La2, Sol2, Fa2, Mi2, Do3

/// Nom d'une note MIDI, sans l'octave.
pub fn note_name(midi: i32) -> &'static str {
    const NOMS: [&str; 12] = [
        "Do", "Do#", "Ré", "Ré#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si",
    ];
    NOMS[midi.rem_euclid(12) as usize]
}

/// Note de la gamme à `step` degrés au-dessus de la tonique.
///
/// `step` peut dépasser 6 ou être négatif : on change alors d'octave. C'est ce qui
/// permet à l'arpège de monter au-delà de l'accord sans jamais sortir de la gamme.
pub fn scale_note(root: i32, step: i32) -> i32 {
    let octave = step.div_euclid(7);
    let index = step.rem_euclid(7) as usize;
    root + octave * 12 + MINOR_SCALE[index]
}

/// Accord construit par superposition de tierces sur un degré.
#[derive(Clone, Debug)]
pub struct Chord {
    pub degree: Degree,
    /// Notes MIDI, de la fondamentale vers l'aigu.
    pub tones: Vec<i32>,
}

impl Chord {
    pub fn root(&self) -> i32 {
        self.tones[0]
    }

    /// Note de l'accord à l'index donné, en montant d'octave si l'on dépasse.
    ///
    /// Sert à l'arpège, qui parcourt l'accord sur plusieurs octaves sans avoir à
    /// connaître sa taille.
    pub fn tone_at(&self, index: usize) -> i32 {
        let n = self.tones.len();
        let octave = (index / n) as i32;
        self.tones[index % n] + octave * 12
    }
}

/// Construit l'accord d'un degré, triade ou septième.
pub fn build_chord(root: i32, degree: Degree, seventh: bool) -> Chord {
    let base = degree.index() as i32;
    let mut tones = vec![
        scale_note(root, base),
        scale_note(root, base + 2),
        scale_note(root, base + 4),
    ];
    if seventh {
        tones.push(scale_note(root, base + 6));
    }
    Chord { degree, tones }
}

/// La note appartient-elle à la gamme mineure de `root` ?
///
/// Réservé aux tests : c'est le prédicat qui vérifie l'invariant du module — rien
/// de ce que produit l'arrangeur ne doit sortir de la gamme. Le code de production
/// n'en a pas besoin, puisque l'invariant est tenu par construction.
#[cfg(test)]
pub fn in_scale(root: i32, midi: i32) -> bool {
    MINOR_SCALE.contains(&(midi - root).rem_euclid(12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_degres_ont_un_index_stable() {
        for (i, d) in Degree::ALL.iter().enumerate() {
            assert_eq!(d.index(), i, "{d:?}");
        }
    }

    #[test]
    fn la_gamme_monte_et_descend_par_octaves() {
        let root = 45; // La2
        assert_eq!(scale_note(root, 0), 45);
        assert_eq!(scale_note(root, 7), 57, "une octave plus haut");
        assert_eq!(scale_note(root, -7), 33, "une octave plus bas");
        assert_eq!(scale_note(root, 2), 48, "tierce mineure");
    }

    /// La propriété qui compte : quel que soit le degré, quelle que soit
    /// l'inversion, aucune note produite ne peut sortir de la gamme.
    #[test]
    fn aucun_accord_ne_sort_de_la_gamme() {
        for root in ROOTS {
            for degree in Degree::ALL {
                for seventh in [false, true] {
                    let accord = build_chord(root, degree, seventh);
                    for note in &accord.tones {
                        assert!(
                            in_scale(root, *note),
                            "{degree:?} sur {root} produit {note}, hors gamme"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn l_arpege_reste_dans_la_gamme_sur_plusieurs_octaves() {
        let root = 45;
        let accord = build_chord(root, Degree::VI, true);
        for i in 0..24 {
            let note = accord.tone_at(i);
            assert!(in_scale(root, note), "index {i} donne {note}, hors gamme");
        }
    }

    #[test]
    fn les_accords_montent_en_hauteur() {
        let accord = build_chord(45, Degree::I, true);
        assert_eq!(accord.tones.len(), 4);
        for paire in accord.tones.windows(2) {
            assert!(paire[1] > paire[0], "accord non ordonné : {:?}", accord.tones);
        }
    }

    #[test]
    fn la_triade_de_tonique_est_bien_mineure() {
        let accord = build_chord(45, Degree::I, false);
        // La – Do – Mi : tierce mineure puis quinte juste.
        assert_eq!(accord.tones, vec![45, 48, 52]);
    }

    #[test]
    fn le_sixieme_degre_est_majeur() {
        // En mineur naturel, VI est un accord majeur : c'est lui qui donne au genre
        // sa couleur douce-amère.
        let accord = build_chord(45, Degree::VI, false);
        let tierce = accord.tones[1] - accord.tones[0];
        assert_eq!(tierce, 4, "tierce majeure attendue");
    }

    #[test]
    fn toutes_les_progressions_font_quatre_mesures() {
        for p in PROGRESSIONS {
            assert_eq!(p.len(), 4);
        }
    }

    #[test]
    fn le_nom_de_note_suit_la_hauteur() {
        assert_eq!(note_name(45), "La");
        assert_eq!(note_name(57), "La");
        assert_eq!(note_name(48), "Do");
    }
}
