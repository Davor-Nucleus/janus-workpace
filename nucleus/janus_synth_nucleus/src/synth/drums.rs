//! Batterie entièrement synthétisée — aucun échantillon sur disque.

use super::filter::OnePoleHighPass;
use super::osc::{Noise, Oscillator, Waveform};

/// Pièce déclenchable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drum {
    Kick,
    Snare,
    HatClosed,
    HatOpen,
}

/// Durée de la coupure nette de la caisse claire.
///
/// La réverbération gatée des années 80 est une queue de réverbération tranchée
/// net. On n'a pas besoin d'une vraie convolution pour l'imiter : une enveloppe
/// coupée brutalement suffit, à condition de ménager un court fondu — sans lui,
/// la coupure produit un clic.
const SNARE_GATE_SECS: f32 = 0.14;
const GATE_FADE_SECS: f32 = 0.004;

pub struct DrumMachine {
    sample_rate: f32,
    noise: Noise,

    kick_osc: Oscillator,
    kick_t: f32,
    kick_on: bool,

    snare_t: f32,
    snare_on: bool,
    snare_osc: Oscillator,

    hat_t: f32,
    hat_on: bool,
    hat_decay: f32,
    hat_hp: OnePoleHighPass,

    /// Passe à `true` sur chaque coup de grosse caisse, pour armer le sidechain.
    kick_triggered: bool,
}

impl DrumMachine {
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            sample_rate,
            noise: Noise::new(seed ^ 0xD8_1234_5678),
            kick_osc: Oscillator::new(sample_rate),
            kick_t: 0.0,
            kick_on: false,
            snare_t: 0.0,
            snare_on: false,
            snare_osc: Oscillator::new(sample_rate),
            hat_t: 0.0,
            hat_on: false,
            hat_decay: 0.012,
            hat_hp: OnePoleHighPass::new(sample_rate, 7_000.0),
            kick_triggered: false,
        }
    }

    pub fn trigger(&mut self, drum: Drum) {
        match drum {
            Drum::Kick => {
                self.kick_t = 0.0;
                self.kick_on = true;
                self.kick_osc.reset();
                self.kick_triggered = true;
            }
            Drum::Snare => {
                self.snare_t = 0.0;
                self.snare_on = true;
                self.snare_osc.reset();
            }
            Drum::HatClosed => {
                self.hat_t = 0.0;
                self.hat_on = true;
                self.hat_decay = 0.012;
            }
            Drum::HatOpen => {
                self.hat_t = 0.0;
                self.hat_on = true;
                self.hat_decay = 0.09;
            }
        }
    }

    /// Consomme le drapeau de déclenchement de grosse caisse.
    pub fn take_kick_trigger(&mut self) -> bool {
        std::mem::replace(&mut self.kick_triggered, false)
    }

    pub fn next(&mut self) -> f32 {
        let dt = 1.0 / self.sample_rate;
        let mut out = 0.0;

        if self.kick_on {
            let t = self.kick_t;
            // La hauteur plonge de 120 à 45 Hz en une cinquantaine de millisecondes :
            // c'est cette chute qui donne l'impact, pas le niveau.
            let freq = 45.0 + 75.0 * (-t / 0.03).exp();
            let amp = (-t / 0.16).exp();
            let body = self.kick_osc.next(freq, Waveform::Sine) * amp;
            // Clic d'attaque très bref, pour percer le mix.
            let click = self.noise.next() * (-t / 0.002).exp() * 0.35;
            out += (body + click) * 0.9;

            self.kick_t += dt;
            if amp < 1e-4 {
                self.kick_on = false;
            }
        }

        if self.snare_on {
            let t = self.snare_t;
            let tone = self.snare_osc.next(185.0, Waveform::Triangle) * (-t / 0.05).exp() * 0.5;
            let body = self.noise.next() * (-t / 0.11).exp();

            // Porte : pleine ouverture, puis fondu court jusqu'à la coupure.
            let gate = if t < SNARE_GATE_SECS {
                1.0
            } else if t < SNARE_GATE_SECS + GATE_FADE_SECS {
                1.0 - (t - SNARE_GATE_SECS) / GATE_FADE_SECS
            } else {
                0.0
            };

            out += (body + tone) * gate * 0.55;

            self.snare_t += dt;
            if gate <= 0.0 {
                self.snare_on = false;
            }
        }

        if self.hat_on {
            let t = self.hat_t;
            let amp = (-t / self.hat_decay).exp();
            out += self.hat_hp.process(self.noise.next()) * amp * 0.3;

            self.hat_t += dt;
            if amp < 1e-4 {
                self.hat_on = false;
            }
        }

        if out.is_finite() { out } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn rendre(machine: &mut DrumMachine, secondes: f32) -> Vec<f32> {
        (0..(FS * secondes) as usize).map(|_| machine.next()).collect()
    }

    #[test]
    fn au_repos_la_batterie_est_silencieuse() {
        let mut m = DrumMachine::new(FS, 1);
        assert!(rendre(&mut m, 0.5).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn chaque_piece_sonne_puis_s_eteint() {
        for drum in [Drum::Kick, Drum::Snare, Drum::HatClosed, Drum::HatOpen] {
            let mut m = DrumMachine::new(FS, 1);
            m.trigger(drum);
            let s = rendre(&mut m, 3.0);

            let energie: f32 = s.iter().map(|x| x.abs()).sum();
            assert!(energie > 0.5, "{drum:?} n'a rien produit");
            assert!(s.iter().all(|x| x.is_finite()), "{drum:?} produit un non-fini");
            assert!(s.iter().all(|x| x.abs() < 3.0), "{drum:?} déborde");

            // Chaque pièce doit finir par se désarmer d'elle-même : sans ce seuil
            // d'extinction, une exponentielle continuerait à être calculée pour
            // toujours et la batterie accumulerait des voix inaudibles.
            let queue = &s[(FS * 2.0) as usize..];
            assert!(
                queue.iter().all(|&x| x == 0.0),
                "{drum:?} sonne encore après 2 s (crête {:.2e})",
                queue.iter().fold(0.0f32, |m, x| m.max(x.abs()))
            );
        }
    }

    /// La coupure nette de la caisse claire est ce qui donne le son « années 80 ».
    #[test]
    fn la_caisse_claire_est_coupee_net() {
        let mut m = DrumMachine::new(FS, 1);
        m.trigger(Drum::Snare);
        let s = rendre(&mut m, 0.5);

        let avant = (SNARE_GATE_SECS * FS) as usize - 100;
        let apres = ((SNARE_GATE_SECS + GATE_FADE_SECS) * FS) as usize + 100;

        assert!(s[avant].abs() > 1e-4, "la claire est déjà éteinte avant la porte");
        assert!(
            s[apres..].iter().all(|x| x.abs() < 1e-6),
            "la porte ne coupe pas"
        );
    }

    #[test]
    fn le_charley_ouvert_dure_plus_que_le_ferme() {
        let duree = |drum: Drum| -> usize {
            let mut m = DrumMachine::new(FS, 1);
            m.trigger(drum);
            rendre(&mut m, 1.0).iter().rposition(|x| x.abs() > 1e-4).unwrap_or(0)
        };
        assert!(duree(Drum::HatOpen) > duree(Drum::HatClosed) * 2);
    }

    #[test]
    fn le_declenchement_de_grosse_caisse_se_consomme_une_seule_fois() {
        let mut m = DrumMachine::new(FS, 1);
        assert!(!m.take_kick_trigger());

        m.trigger(Drum::Kick);
        assert!(m.take_kick_trigger(), "déclenchement non signalé");
        assert!(!m.take_kick_trigger(), "déclenchement signalé deux fois");
    }

    #[test]
    fn les_pieces_superposees_restent_bornees() {
        let mut m = DrumMachine::new(FS, 1);
        m.trigger(Drum::Kick);
        m.trigger(Drum::Snare);
        m.trigger(Drum::HatOpen);
        for s in rendre(&mut m, 1.0) {
            assert!(s.is_finite() && s.abs() < 3.0, "superposition hors bornes : {s}");
        }
    }
}
