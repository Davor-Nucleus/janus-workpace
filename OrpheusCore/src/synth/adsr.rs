//! Enveloppe ADSR.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Réglages d'une enveloppe, en secondes (sauf `sustain`, un niveau).
#[derive(Clone, Copy, Debug)]
pub struct AdsrSettings {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl AdsrSettings {
    pub const fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack,
            decay,
            sustain,
            release,
        }
    }
}

/// Enveloppe à attaque linéaire et décroissances exponentielles.
///
/// Le decay et le release sont exponentiels parce qu'une décroissance linéaire
/// s'entend comme une coupure : l'oreille perçoit le niveau en décibels, donc
/// une droite en amplitude paraît plonger d'un coup à la fin.
#[derive(Clone)]
pub struct Adsr {
    stage: Stage,
    value: f32,
    sample_rate: f32,
    settings: AdsrSettings,
}

/// Seuil sous lequel on considère l'enveloppe éteinte.
///
/// Une exponentielle n'atteint jamais zéro : sans ce seuil, aucune voix ne serait
/// jamais libérée et le mixeur accumulerait des voix inaudibles indéfiniment.
const EPSILON: f32 = 1e-4;

impl Adsr {
    pub fn new(sample_rate: f32, settings: AdsrSettings) -> Self {
        Self {
            stage: Stage::Idle,
            value: 0.0,
            sample_rate,
            settings,
        }
    }

    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Coupe net, sans release. Réservé au vol de voix.
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.value = 0.0;
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Réservé aux tests : la production consomme la valeur via [`Self::next`].
    #[cfg(test)]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Coefficient d'un lissage à un pôle atteignant ~99 % de sa cible en `secs`.
    fn coef(&self, secs: f32) -> f32 {
        let n = (secs.max(1e-4)) * self.sample_rate;
        1.0 - (-4.6 / n).exp()
    }

    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => self.value = 0.0,
            Stage::Attack => {
                let inc = 1.0 / (self.settings.attack.max(1e-4) * self.sample_rate);
                self.value += inc;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let c = self.coef(self.settings.decay);
                self.value += (self.settings.sustain - self.value) * c;
                if (self.value - self.settings.sustain).abs() < EPSILON {
                    self.value = self.settings.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.value = self.settings.sustain,
            Stage::Release => {
                let c = self.coef(self.settings.release);
                self.value -= self.value * c;
                if self.value < EPSILON {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn env(a: f32, d: f32, s: f32, r: f32) -> Adsr {
        Adsr::new(FS, AdsrSettings::new(a, d, s, r))
    }

    #[test]
    fn au_repos_elle_reste_a_zero() {
        let mut e = env(0.01, 0.1, 0.5, 0.1);
        assert!(!e.is_active());
        for _ in 0..1_000 {
            assert_eq!(e.next(), 0.0);
        }
    }

    #[test]
    fn elle_monte_puis_se_stabilise_sur_le_sustain() {
        let mut e = env(0.01, 0.05, 0.5, 0.1);
        e.trigger();
        // 0,5 s : largement de quoi traverser attaque et decay.
        for _ in 0..(FS as usize / 2) {
            e.next();
        }
        assert!((e.value() - 0.5).abs() < 1e-3, "sustain non atteint : {}", e.value());
    }

    #[test]
    fn elle_ne_depasse_jamais_un() {
        let mut e = env(0.001, 0.2, 1.0, 0.1);
        e.trigger();
        for _ in 0..(FS as usize) {
            let v = e.next();
            assert!((0.0..=1.0).contains(&v), "valeur hors bornes : {v}");
        }
    }

    /// Sans seuil d'extinction, une exponentielle n'atteindrait jamais zéro et la
    /// voix resterait comptée comme active pour toujours.
    #[test]
    fn le_release_libere_effectivement_la_voix() {
        let mut e = env(0.001, 0.01, 0.8, 0.05);
        e.trigger();
        for _ in 0..(FS as usize / 10) {
            e.next();
        }
        e.release();
        for _ in 0..(FS as usize) {
            e.next();
        }
        assert!(!e.is_active(), "voix jamais libérée");
        assert_eq!(e.value(), 0.0);
    }

    #[test]
    fn un_release_avant_declenchement_est_sans_effet() {
        let mut e = env(0.01, 0.1, 0.5, 0.1);
        e.release();
        assert!(!e.is_active());
    }

    #[test]
    fn les_valeurs_restent_finies_sur_des_reglages_extremes() {
        let mut e = env(0.0, 0.0, 0.0, 0.0);
        e.trigger();
        for _ in 0..10_000 {
            assert!(e.next().is_finite());
        }
    }
}
