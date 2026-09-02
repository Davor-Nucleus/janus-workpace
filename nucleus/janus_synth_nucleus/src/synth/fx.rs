//! Chaîne d'effets et de sortie.

/// Ligne à retard simple, à interpolation nulle (le temps est fixé au démarrage).
struct DelayLine {
    buffer: Vec<f32>,
    index: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
        }
    }

    fn read(&self) -> f32 {
        self.buffer[self.index]
    }

    fn write(&mut self, value: f32) {
        self.buffer[self.index] = if value.is_finite() { value } else { 0.0 };
        self.index = (self.index + 1) % self.buffer.len();
    }
}

/// Delay ping-pong : les répétitions rebondissent d'une oreille à l'autre.
///
/// Calé sur la croche pointée, l'intervalle qui donne au genre son balancement.
pub struct PingPongDelay {
    left: DelayLine,
    right: DelayLine,
    feedback: f32,
    mix: f32,
}

impl PingPongDelay {
    pub fn new(sample_rate: f32, delay_secs: f32, feedback: f32, mix: f32) -> Self {
        let len = (delay_secs * sample_rate) as usize;
        Self {
            left: DelayLine::new(len),
            right: DelayLine::new(len),
            feedback: feedback.clamp(0.0, 0.95),
            mix: mix.clamp(0.0, 1.0),
        }
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let dl = self.left.read();
        let dr = self.right.read();

        // Le croisement gauche/droite est ce qui fait le « ping-pong ».
        self.left.write(r + dr * self.feedback);
        self.right.write(l + dl * self.feedback);

        (l + dl * self.mix, r + dr * self.mix)
    }
}

/// Filtre en peigne avec amortissement, brique de la réverbération.
struct Comb {
    line: DelayLine,
    feedback: f32,
    damp: f32,
    filter_store: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32, damp: f32) -> Self {
        Self {
            line: DelayLine::new(len),
            feedback,
            damp,
            filter_store: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.line.read();
        // L'amortissement absorbe les aigus à chaque tour : sans lui la queue
        // devient métallique et sifflante.
        self.filter_store = out * (1.0 - self.damp) + self.filter_store * self.damp;
        self.line.write(input + self.filter_store * self.feedback);
        out
    }
}

/// Passe-tout, qui densifie les échos sans colorer le spectre.
struct AllPass {
    line: DelayLine,
    feedback: f32,
}

impl AllPass {
    fn new(len: usize, feedback: f32) -> Self {
        Self {
            line: DelayLine::new(len),
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.line.read();
        self.line.write(input + buffered * self.feedback);
        buffered - input
    }
}

/// Réverbération de Schroeder : quatre peignes en parallèle, deux passe-tout en série.
///
/// Les longueurs sont premières entre elles pour que les résonances des peignes ne
/// se superposent pas — sinon la queue sonne comme un tuyau.
pub struct Reverb {
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    allpass_l: Vec<AllPass>,
    allpass_r: Vec<AllPass>,
    mix: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32, mix: f32) -> Self {
        // Longueurs de Freeverb, transposées de 44,1 à la fréquence courante.
        let scale = sample_rate / 44_100.0;
        let comb_lens = [1_116.0, 1_188.0, 1_277.0, 1_356.0];
        let allpass_lens = [556.0, 441.0];
        // Décalage stéréo : les deux canaux ne doivent pas être identiques.
        let spread = (23.0 * scale) as usize;

        let mk_combs = |offset: usize| {
            comb_lens
                .iter()
                .map(|l| Comb::new((l * scale) as usize + offset, 0.84, 0.25))
                .collect::<Vec<_>>()
        };
        let mk_allpass = |offset: usize| {
            allpass_lens
                .iter()
                .map(|l| AllPass::new((l * scale) as usize + offset, 0.5))
                .collect::<Vec<_>>()
        };

        Self {
            combs_l: mk_combs(0),
            combs_r: mk_combs(spread),
            allpass_l: mk_allpass(0),
            allpass_r: mk_allpass(spread),
            mix: mix.clamp(0.0, 1.0),
        }
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mut wet_l: f32 = self.combs_l.iter_mut().map(|c| c.process(l)).sum::<f32>() * 0.25;
        let mut wet_r: f32 = self.combs_r.iter_mut().map(|c| c.process(r)).sum::<f32>() * 0.25;

        for ap in &mut self.allpass_l {
            wet_l = ap.process(wet_l);
        }
        for ap in &mut self.allpass_r {
            wet_r = ap.process(wet_r);
        }

        (l + wet_l * self.mix, r + wet_r * self.mix)
    }
}

/// Ducking déclenché par la grosse caisse.
///
/// Le « pompage » est une signature du genre : à chaque kick, nappe, basse et
/// arpège plongent puis remontent. C'est ce qui fait respirer le mixage et donne
/// sa place à la grosse caisse sans monter son niveau.
pub struct Sidechain {
    envelope: f32,
    release_coef: f32,
    amount: f32,
}

impl Sidechain {
    pub fn new(sample_rate: f32, release_secs: f32, amount: f32) -> Self {
        Self {
            envelope: 0.0,
            release_coef: 1.0 - (-4.6 / (release_secs.max(1e-3) * sample_rate)).exp(),
            amount: amount.clamp(0.0, 1.0),
        }
    }

    pub fn trigger(&mut self) {
        self.envelope = 1.0;
    }

    /// Gain à appliquer pour cet échantillon, dans `[1 − amount, 1]`.
    pub fn next_gain(&mut self) -> f32 {
        let gain = 1.0 - self.envelope * self.amount;
        self.envelope -= self.envelope * self.release_coef;
        if self.envelope < 1e-5 {
            self.envelope = 0.0;
        }
        gain
    }
}

/// Écrêtage doux, dernier étage avant la conversion en entier.
///
/// `tanh` borne strictement la sortie à ±1 tout en restant lisse : la somme d'une
/// dizaine de voix dépasse largement l'unité, et un `clamp` franc produirait une
/// distorsion sale à chaque crête.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x.is_finite() { x.tanh() } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::osc::Noise;

    const FS: f32 = 48_000.0;

    #[test]
    fn l_ecretage_borne_toujours_la_sortie() {
        for x in [-1e9, -3.0, -1.0, 0.0, 0.5, 3.0, 1e9] {
            let y = soft_clip(x);
            assert!(y.abs() <= 1.0, "{x} donne {y}");
        }
        // Et surtout : un non-fini ne doit jamais traverser l'étage de sortie.
        assert_eq!(soft_clip(f32::NAN), 0.0);
        assert_eq!(soft_clip(f32::INFINITY), 0.0);
    }

    #[test]
    fn l_ecretage_est_transparent_a_bas_niveau() {
        // En dessous de −20 dBFS, la déformation doit rester imperceptible.
        for x in [-0.1, -0.01, 0.01, 0.1] {
            assert!((soft_clip(x) - x).abs() < 0.005, "{x} trop déformé");
        }
    }

    #[test]
    fn le_delay_reste_stable_en_boucle_longue() {
        let mut d = PingPongDelay::new(FS, 0.35, 0.9, 0.4);
        let mut bruit = Noise::new(5);
        for i in 0..(FS as usize * 10) {
            // Impulsion au départ, puis silence : la boucle doit s'éteindre.
            let x = if i < 100 { bruit.next() } else { 0.0 };
            let (l, r) = d.process(x, x);
            assert!(l.is_finite() && r.is_finite(), "delay divergent");
            assert!(l.abs() < 10.0 && r.abs() < 10.0, "delay en emballement");
        }
    }

    #[test]
    fn le_delay_croise_bien_les_canaux() {
        let mut d = PingPongDelay::new(FS, 0.01, 0.5, 1.0);
        // Impulsion à gauche seulement.
        d.process(1.0, 0.0);
        let mut vu_a_droite = false;
        for _ in 0..(FS as usize / 50) {
            let (_, r) = d.process(0.0, 0.0);
            if r.abs() > 1e-3 {
                vu_a_droite = true;
                break;
            }
        }
        assert!(vu_a_droite, "le signal n'a jamais traversé vers la droite");
    }

    #[test]
    fn la_reverberation_reste_stable_et_decroit() {
        let mut rev = Reverb::new(FS, 0.4);
        let mut bruit = Noise::new(9);
        let mut queue = 0.0f32;

        for i in 0..(FS as usize * 8) {
            let x = if i < (FS as usize / 10) { bruit.next() * 0.5 } else { 0.0 };
            let (l, r) = rev.process(x, x);
            assert!(l.is_finite() && r.is_finite(), "réverbération divergente");
            assert!(l.abs() < 10.0, "réverbération en emballement : {l}");
            if i > FS as usize * 6 {
                queue += l.abs();
            }
        }
        assert!(queue < 1.0, "la queue ne décroît pas : {queue}");
    }

    #[test]
    fn la_reverberation_decorrele_les_canaux() {
        let mut rev = Reverb::new(FS, 1.0);
        let mut bruit = Noise::new(13);
        let mut differences = 0;
        for _ in 0..(FS as usize) {
            let x = bruit.next();
            let (l, r) = rev.process(x, x);
            if (l - r).abs() > 1e-4 {
                differences += 1;
            }
        }
        assert!(differences > 1_000, "les deux canaux sont identiques");
    }

    #[test]
    fn le_sidechain_plonge_puis_remonte() {
        let mut sc = Sidechain::new(FS, 0.25, 0.7);
        assert!((sc.next_gain() - 1.0).abs() < 1e-6, "gain initial incorrect");

        sc.trigger();
        let creux = sc.next_gain();
        assert!((creux - 0.3).abs() < 0.01, "profondeur du ducking : {creux}");

        for _ in 0..(FS as usize) {
            sc.next_gain();
        }
        assert!((sc.next_gain() - 1.0).abs() < 1e-3, "le gain n'est pas remonté");
    }

    #[test]
    fn le_gain_du_sidechain_reste_dans_ses_bornes() {
        let mut sc = Sidechain::new(FS, 0.2, 0.6);
        for i in 0..(FS as usize) {
            if i % 12_000 == 0 {
                sc.trigger();
            }
            let g = sc.next_gain();
            assert!((0.4 - 1e-6..=1.0).contains(&g), "gain hors bornes : {g}");
        }
    }
}
