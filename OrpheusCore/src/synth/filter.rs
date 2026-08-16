//! Filtres.

use std::f32::consts::PI;

/// Filtre résonant à variables d'état, forme TPT.
///
/// C'est le cœur du son du genre : un passe-bas résonant qui balaie. La forme
/// employée est celle à **transformation préservant la topologie** (intégration
/// trapézoïdale), et non la forme de Chamberlin plus répandue.
///
/// La raison est concrète. Chamberlin utilise `f = 2·sin(π·fc/fs)` et n'est fidèle
/// que bien en dessous de Nyquist : au-delà d'environ `fs/6`, sa condition de
/// stabilité se resserre jusqu'à croiser l'amortissement demandé, et un balayage
/// de coupure à forte résonance voit son gain croître sans borne. La forme TPT est
/// inconditionnellement stable sur tout le spectre et reste juste jusqu'à Nyquist,
/// pour deux multiplications de plus.
///
/// L'enjeu n'est pas cosmétique : une divergence produit un `NaN` qui contamine le
/// delay et la réverbération en aval, dont il ne ressortira jamais.
#[derive(Clone)]
pub struct StateVariableFilter {
    sample_rate: f32,
    /// États des deux intégrateurs.
    ic1: f32,
    ic2: f32,
}

/// Plafond de coupure, en fraction de la fréquence d'échantillonnage.
///
/// `tan(π·fc/fs)` diverge en `fs/2` ; s'en tenir à l'écart garde `g` fini. À cette
/// coupure le passe-bas est de toute façon grand ouvert.
const MAX_CUTOFF_RATIO: f32 = 0.49;

/// Plafond de résonance, exprimé en facteur de qualité maximal.
///
/// Le gain de crête d'un passe-bas résonant vaut environ Q. Le laisser monter à
/// plusieurs dizaines écraserait le limiteur à la moindre pointe ; 10 suffit
/// largement à faire chanter un balayage.
const MAX_Q: f32 = 10.0;

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ic1: 0.0,
            ic2: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Filtre un échantillon en passe-bas.
    ///
    /// `resonance` va de 0 (aucune) à 1 (Q maximal). La structure calcule aussi les
    /// sorties passe-haut et passe-bande, mais aucune voix ne s'en sert : les
    /// exposer serait du code mort, et elles se réajoutent en trois lignes.
    pub fn process(&mut self, input: f32, cutoff_hz: f32, resonance: f32) -> f32 {
        if !input.is_finite() {
            self.reset();
            return 0.0;
        }

        let max_cutoff = self.sample_rate * MAX_CUTOFF_RATIO;
        let cutoff = cutoff_hz.clamp(20.0, max_cutoff);
        let resonance = resonance.clamp(0.0, 1.0);

        let g = (PI * cutoff / self.sample_rate).tan();
        let q = 0.5 + resonance * (MAX_Q - 0.5);
        let k = 1.0 / q;

        // Compensation d'entrée : monter la résonance ne doit pas faire enfler le
        // niveau global, sinon un balayage de filtre s'entend d'abord comme un
        // changement de volume et vient taper dans le limiteur.
        let input = input / (1.0 + resonance * 3.0);

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        // Ceinture et bretelles : si l'état a malgré tout dégénéré, on repart de zéro
        // plutôt que de propager un NaN dans toute la chaîne aval.
        if !self.ic1.is_finite() || !self.ic2.is_finite() {
            self.reset();
            return 0.0;
        }

        v2
    }
}

/// Passe-haut à un pôle, pour les charleys et le nettoyage des graves.
#[derive(Clone)]
pub struct OnePoleHighPass {
    prev_in: f32,
    prev_out: f32,
    coef: f32,
}

impl OnePoleHighPass {
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let rc = 1.0 / (2.0 * PI * cutoff_hz.max(1.0));
        let dt = 1.0 / sample_rate;
        Self {
            prev_in: 0.0,
            prev_out: 0.0,
            coef: rc / (rc + dt),
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let out = self.coef * (self.prev_out + input - self.prev_in);
        self.prev_in = input;
        self.prev_out = if out.is_finite() { out } else { 0.0 };
        self.prev_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::osc::Noise;

    const FS: f32 = 48_000.0;

    /// Le test central : à résonance maximale, sur tout le balayage de coupure, la
    /// sortie doit rester finie et bornée. C'est exactement le cas qui fait
    /// diverger un SVF écrit sans borne d'amortissement.
    #[test]
    fn reste_stable_a_resonance_maximale_sur_tout_le_balayage() {
        let mut bruit = Noise::new(7);
        let mut filtre = StateVariableFilter::new(FS);

        let n = FS as usize * 2;
        for i in 0..n {
            // Coupure balayée de 20 Hz à Nyquist, résonance au maximum.
            let t = i as f32 / n as f32;
            let cutoff = 20.0 + t * (FS / 2.0);
            let out = filtre.process(bruit.next(), cutoff, 1.0);

            assert!(out.is_finite(), "divergence à {cutoff:.0} Hz (échantillon {i})");
            // Au plafond de résonance, Q vaut 10 et la compensation d'entrée divise
            // par 4 : le pic attendu est de l'ordre de 2,5, transitoires du bruit
            // blanc comprises. La forme de Chamberlin, elle, dépassait 20 ici et
            // continuait de croître avec la coupure.
            assert!(out.abs() < 5.0, "gain hors de contrôle à {cutoff:.0} Hz : {out}");
        }
    }

    /// Le niveau ne doit pas enfler quand on monte la résonance : sinon un
    /// balayage de filtre s'entend comme un changement de volume.
    #[test]
    fn la_resonance_ne_fait_pas_enfler_le_niveau() {
        let rms = |resonance: f32| -> f32 {
            let mut bruit = Noise::new(21);
            let mut filtre = StateVariableFilter::new(FS);
            let n = FS as usize;
            let somme: f32 = (0..n)
                .map(|_| {
                    let s = filtre.process(bruit.next(), 1_500.0, resonance);
                    s * s
                })
                .sum();
            (somme / n as f32).sqrt()
        };

        let sec = rms(0.0);
        let resonant = rms(0.9);
        assert!(
            resonant < sec * 2.0,
            "la résonance multiplie le niveau par {:.1}",
            resonant / sec
        );
    }

    /// Un passe-bas doit atténuer nettement une sinusoïde très au-dessus de sa
    /// coupure, et laisser passer celle qui est en dessous.
    #[test]
    fn le_passe_bas_attenue_bien_les_aigus() {
        let rms = |freq: f32, cutoff: f32| -> f32 {
            let mut osc = crate::synth::osc::Oscillator::new(FS);
            let mut filtre = StateVariableFilter::new(FS);
            let n = FS as usize / 2;
            let mut somme = 0.0;
            for i in 0..n {
                let s = osc.next(freq, crate::synth::osc::Waveform::Sine);
                let out = filtre.process(s, cutoff, 0.0);
                // On ignore le régime transitoire du début.
                if i > n / 4 {
                    somme += out * out;
                }
            }
            (somme / (n as f32 * 0.75)).sqrt()
        };

        let passante = rms(200.0, 2_000.0);
        let coupee = rms(12_000.0, 2_000.0);
        assert!(
            coupee < passante * 0.2,
            "atténuation insuffisante : passante={passante:.4}, coupée={coupee:.4}"
        );
    }

    #[test]
    fn une_entree_non_finie_ne_contamine_pas_le_filtre() {
        let mut filtre = StateVariableFilter::new(FS);
        let _ = filtre.process(f32::NAN, 1_000.0, 0.5);
        // L'état doit avoir été purgé : les échantillons suivants redeviennent sains.
        for _ in 0..10 {
            assert!(filtre.process(0.5, 1_000.0, 0.5).is_finite());
        }
    }

    #[test]
    fn le_passe_haut_a_un_pole_reste_fini() {
        let mut bruit = Noise::new(3);
        let mut hp = OnePoleHighPass::new(FS, 7_000.0);
        for _ in 0..(FS as usize) {
            assert!(hp.process(bruit.next()).is_finite());
        }
    }
}
