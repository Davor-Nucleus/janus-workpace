//! Oscillateurs et bruit.

use std::f32::consts::TAU;

/// Correction PolyBLEP autour d'une discontinuité.
///
/// Une dent de scie naïve (`2·phase − 1`) contient des harmoniques jusqu'à
/// l'infini. Au-delà de Nyquist elles se replient en fréquences inharmoniques :
/// c'est ce qui donne le timbre métallique et sale des synthés écrits vite, et
/// c'est d'autant plus audible que la note est aiguë.
///
/// PolyBLEP remplace le saut brutal par un segment polynomial de deux
/// échantillons, ce qui supprime l'essentiel du repliement pour un coût dérisoire.
/// `t` est la phase dans `[0,1)`, `dt` l'avance de phase par échantillon.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        // Juste après le saut.
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        // Juste avant le saut.
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Forme d'onde d'un oscillateur.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
}

/// Oscillateur à phase accumulée.
#[derive(Clone)]
pub struct Oscillator {
    phase: f32,
    sample_rate: f32,
}

impl Oscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            sample_rate,
        }
    }

    /// Décale la phase de départ, pour désaccorder deux oscillateurs sans qu'ils
    /// démarrent en phase — sinon leur somme claque à l'attaque.
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase.rem_euclid(1.0);
        self
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Produit l'échantillon suivant à la fréquence `freq`.
    pub fn next(&mut self, freq: f32, waveform: Waveform) -> f32 {
        let dt = (freq / self.sample_rate).clamp(0.0, 0.5);
        let phase = self.phase;

        let value = match waveform {
            // Sine et triangle sont continues : pas de discontinuité à corriger.
            Waveform::Sine => (phase * TAU).sin(),
            Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            Waveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
            Waveform::Square => {
                let naive = if phase < 0.5 { 1.0 } else { -1.0 };
                // Deux discontinuités par période : la montée en 0, la descente en 0,5.
                naive + poly_blep(phase, dt) - poly_blep((phase + 0.5).rem_euclid(1.0), dt)
            }
        };

        self.phase = (self.phase + dt).rem_euclid(1.0);
        value
    }
}

/// Bruit blanc déterministe (xorshift64*).
///
/// Générateur propre plutôt que `rand` : le bruit est tiré à chaque échantillon
/// des charleys et de la caisse claire, et il doit être reproductible pour une
/// graine donnée sans dépendre de l'ordre des autres tirages.
#[derive(Clone)]
pub struct Noise {
    state: u64,
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self {
            // Un état nul bloquerait le xorshift sur zéro.
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// Échantillon dans `[-1, 1)`.
    pub fn next(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        // 24 bits de mantisse : suffisant, et évite les valeurs non représentables.
        ((self.state >> 40) as f32 / 8_388_608.0) - 1.0
    }
}

/// Conversion note MIDI → fréquence (La 440 = note 69).
#[inline]
pub fn midi_to_freq(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Désaccord en cents appliqué à une fréquence.
#[inline]
pub fn detune(freq: f32, cents: f32) -> f32 {
    freq * (cents / 1200.0).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn rendre(waveform: Waveform, freq: f32, n: usize) -> Vec<f32> {
        let mut osc = Oscillator::new(FS);
        (0..n).map(|_| osc.next(freq, waveform)).collect()
    }

    #[test]
    fn les_ondes_restent_dans_les_bornes() {
        for w in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Square,
        ] {
            for freq in [55.0, 440.0, 3_000.0] {
                for s in rendre(w, freq, 4_800) {
                    assert!(s.is_finite(), "{w:?} à {freq} Hz produit un non-fini");
                    assert!(s.abs() <= 1.5, "{w:?} à {freq} Hz déborde : {s}");
                }
            }
        }
    }

    #[test]
    fn la_frequence_produite_est_la_bonne() {
        // Une seconde de sinus à 440 Hz traverse zéro 880 fois (deux fois par période).
        let s = rendre(Waveform::Sine, 440.0, FS as usize);
        let croisements = s.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!(
            (croisements as i32 - 440).abs() <= 1,
            "440 périodes attendues, {croisements} mesurées"
        );
    }

    #[test]
    fn la_dent_de_scie_n_a_pas_d_offset_continu() {
        let s = rendre(Waveform::Saw, 100.0, FS as usize);
        let moyenne: f32 = s.iter().sum::<f32>() / s.len() as f32;
        assert!(moyenne.abs() < 0.01, "offset DC de {moyenne}");
    }

    /// Le test qui justifie PolyBLEP, par sa définition même.
    ///
    /// Le repliement, c'est l'écart entre l'onde produite et la dent de scie
    /// **idéale à bande limitée** — celle qui ne contient que les harmoniques
    /// tenant sous Nyquist, et qu'on peut donc synthétiser exactement par addition
    /// de sinusoïdes. On mesure l'erreur quadratique des deux implémentations par
    /// rapport à cette référence : PolyBLEP doit être nettement plus proche.
    #[test]
    fn polyblep_reduit_le_repliement() {
        let freq = 3_000.0;
        let n = 4_096;
        let harmoniques = (FS / 2.0 / freq) as usize; // 8 sous Nyquist

        // Référence : -(2/π)·Σ sin(2π·k·f·t)/k, même phase que `2·phase − 1`.
        let ideale: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / FS;
                let somme: f32 = (1..=harmoniques)
                    .map(|k| (TAU * k as f32 * freq * t).sin() / k as f32)
                    .sum();
                -(2.0 / std::f32::consts::PI) * somme
            })
            .collect();

        let naive: Vec<f32> = {
            let mut phase = 0.0f32;
            let dt = freq / FS;
            (0..n)
                .map(|_| {
                    let v = 2.0 * phase - 1.0;
                    phase = (phase + dt).rem_euclid(1.0);
                    v
                })
                .collect()
        };
        let corrigee = rendre(Waveform::Saw, freq, n);

        let erreur = |s: &[f32]| -> f32 {
            let somme: f32 = s
                .iter()
                .zip(&ideale)
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            (somme / n as f32).sqrt()
        };

        let e_corrigee = erreur(&corrigee);
        let e_naive = erreur(&naive);
        assert!(
            e_corrigee < e_naive * 0.75,
            "PolyBLEP n'apporte rien : corrigée={e_corrigee:.4}, naïve={e_naive:.4}"
        );
    }

    #[test]
    fn le_bruit_est_borne_et_reproductible() {
        let mut a = Noise::new(42);
        let mut b = Noise::new(42);
        for _ in 0..10_000 {
            let x = a.next();
            assert!(x.is_finite() && (-1.0..1.0).contains(&x), "bruit hors bornes : {x}");
            assert_eq!(x, b.next(), "même graine, suites différentes");
        }
    }

    #[test]
    fn le_bruit_ne_se_bloque_pas_sur_une_graine_nulle() {
        let mut n = Noise::new(0);
        let premiers: Vec<f32> = (0..8).map(|_| n.next()).collect();
        assert!(premiers.iter().any(|&x| x != premiers[0]));
    }

    #[test]
    fn conversion_midi_vers_frequence() {
        assert!((midi_to_freq(69.0) - 440.0).abs() < 0.01);
        assert!((midi_to_freq(57.0) - 220.0).abs() < 0.01);
        assert!((detune(440.0, 1200.0) - 880.0).abs() < 0.01);
    }
}
