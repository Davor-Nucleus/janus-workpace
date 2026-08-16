//! Voix polyphoniques : oscillateurs désaccordés, enveloppes, filtre.

use super::adsr::{Adsr, AdsrSettings};
use super::filter::StateVariableFilter;
use super::osc::{detune, midi_to_freq, Oscillator, Waveform};

/// Recette sonore d'un instrument.
#[derive(Clone, Copy)]
pub struct VoicePreset {
    pub waveform: Waveform,
    /// Désaccords en cents, un oscillateur par entrée. La largeur du son en dépend.
    pub detune_cents: &'static [f32],
    /// Sous-oscillateur sinusoïdal à l'octave inférieure, pour asseoir les graves.
    pub sub_level: f32,
    pub amp: AdsrSettings,
    pub filter_env: AdsrSettings,
    /// Coupure au repos, et amplitude de l'enveloppe de filtre en Hz.
    pub cutoff_base: f32,
    pub cutoff_env: f32,
    pub resonance: f32,
    /// Temps de glissando entre deux notes, en secondes. 0 = attaque franche.
    pub portamento: f32,
}

/// Nappe : large, lente, c'est le lit harmonique du morceau.
pub const PAD: VoicePreset = VoicePreset {
    waveform: Waveform::Saw,
    detune_cents: &[-7.0, 0.0, 7.0],
    sub_level: 0.3,
    amp: AdsrSettings::new(0.45, 1.2, 0.7, 1.6),
    filter_env: AdsrSettings::new(1.5, 2.0, 0.6, 1.5),
    cutoff_base: 400.0,
    cutoff_env: 1_800.0,
    resonance: 0.25,
    portamento: 0.0,
};

/// Basse : mono, courte, avec une enveloppe de filtre marquée qui lui donne son mordant.
pub const BASS: VoicePreset = VoicePreset {
    waveform: Waveform::Saw,
    detune_cents: &[0.0],
    sub_level: 0.6,
    amp: AdsrSettings::new(0.005, 0.18, 0.35, 0.08),
    filter_env: AdsrSettings::new(0.002, 0.12, 0.2, 0.08),
    cutoff_base: 180.0,
    cutoff_env: 1_400.0,
    resonance: 0.4,
    portamento: 0.0,
};

/// Arpège : pincé, court, il occupe la grille en doubles-croches.
pub const ARP: VoicePreset = VoicePreset {
    waveform: Waveform::Square,
    detune_cents: &[-4.0, 4.0],
    sub_level: 0.0,
    amp: AdsrSettings::new(0.003, 0.15, 0.0, 0.1),
    filter_env: AdsrSettings::new(0.002, 0.14, 0.0, 0.1),
    cutoff_base: 700.0,
    cutoff_env: 3_500.0,
    resonance: 0.5,
    portamento: 0.0,
};

/// Lead : deux scies désaccordées et un glissando, la signature du genre.
pub const LEAD: VoicePreset = VoicePreset {
    waveform: Waveform::Saw,
    detune_cents: &[-11.0, 11.0],
    sub_level: 0.15,
    amp: AdsrSettings::new(0.02, 0.4, 0.6, 0.5),
    filter_env: AdsrSettings::new(0.05, 0.5, 0.5, 0.5),
    cutoff_base: 900.0,
    cutoff_env: 4_000.0,
    resonance: 0.35,
    portamento: 0.06,
};

/// Une voix monophonique complète.
#[derive(Clone)]
pub struct Voice {
    preset: VoicePreset,
    oscillators: Vec<Oscillator>,
    sub: Oscillator,
    amp_env: Adsr,
    filter_env: Adsr,
    filter: StateVariableFilter,
    /// Fréquence visée, et fréquence courante lissée par le portamento.
    target_freq: f32,
    current_freq: f32,
    porta_coef: f32,
    /// Ordre de déclenchement, pour voler la voix la plus ancienne.
    pub age: u64,
}

impl Voice {
    pub fn new(sample_rate: f32, preset: VoicePreset) -> Self {
        let oscillators = preset
            .detune_cents
            .iter()
            .enumerate()
            // Phases de départ décalées : deux oscillateurs démarrant en phase
            // s'additionnent en un claquement à l'attaque.
            .map(|(i, _)| {
                Oscillator::new(sample_rate).with_phase(i as f32 * 0.37)
            })
            .collect();

        let porta_coef = if preset.portamento > 0.0 {
            1.0 - (-4.6 / (preset.portamento * sample_rate)).exp()
        } else {
            1.0
        };

        Self {
            preset,
            oscillators,
            sub: Oscillator::new(sample_rate),
            amp_env: Adsr::new(sample_rate, preset.amp),
            filter_env: Adsr::new(sample_rate, preset.filter_env),
            filter: StateVariableFilter::new(sample_rate),
            target_freq: 440.0,
            current_freq: 440.0,
            porta_coef,
            age: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.amp_env.is_active()
    }

    /// Déclenche une note MIDI.
    pub fn note_on(&mut self, midi_note: f32, age: u64) {
        self.target_freq = midi_to_freq(midi_note);
        // Sans portamento, on saute directement : sinon la première note glisserait
        // depuis la fréquence de la note précédente, ce qui s'entend comme un bug.
        if self.preset.portamento <= 0.0 || !self.amp_env.is_active() {
            self.current_freq = self.target_freq;
        }
        self.amp_env.trigger();
        self.filter_env.trigger();
        self.age = age;
    }

    pub fn note_off(&mut self) {
        self.amp_env.release();
        self.filter_env.release();
    }

    pub fn steal(&mut self) {
        self.amp_env.reset();
        self.filter_env.reset();
        self.filter.reset();
    }

    /// Produit l'échantillon suivant. `cutoff_scale` module la coupure globalement
    /// (c'est par là que l'arrangeur ouvre et referme le filtre au fil des sections).
    pub fn next(&mut self, cutoff_scale: f32) -> f32 {
        let amp = self.amp_env.next();
        if !self.amp_env.is_active() {
            return 0.0;
        }

        self.current_freq += (self.target_freq - self.current_freq) * self.porta_coef;
        let freq = self.current_freq;

        let mut sum = 0.0;
        for (osc, cents) in self
            .oscillators
            .iter_mut()
            .zip(self.preset.detune_cents.iter())
        {
            sum += osc.next(detune(freq, *cents), self.preset.waveform);
        }
        sum /= self.oscillators.len() as f32;

        if self.preset.sub_level > 0.0 {
            sum += self.sub.next(freq * 0.5, Waveform::Sine) * self.preset.sub_level;
        }

        let fenv = self.filter_env.next();
        let cutoff = (self.preset.cutoff_base + self.preset.cutoff_env * fenv) * cutoff_scale;
        let filtered = self.filter.process(sum, cutoff, self.preset.resonance);

        filtered * amp
    }
}

/// Un instrument polyphonique : plusieurs voix partageant un même préréglage.
pub struct Instrument {
    voices: Vec<Voice>,
    counter: u64,
}

impl Instrument {
    pub fn new(sample_rate: f32, preset: VoicePreset, polyphony: usize) -> Self {
        Self {
            voices: (0..polyphony.max(1))
                .map(|_| Voice::new(sample_rate, preset))
                .collect(),
            counter: 0,
        }
    }

    /// Joue une note sur une voix libre, ou vole la plus ancienne.
    ///
    /// Sans vol de voix, une nappe saturée avalerait silencieusement les nouveaux
    /// accords : le changement d'harmonie ne s'entendrait tout simplement pas.
    pub fn note_on(&mut self, midi_note: f32) {
        self.counter += 1;
        let age = self.counter;

        let index = match self.voices.iter().position(|v| !v.is_active()) {
            Some(i) => i,
            None => {
                let i = self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.voices[i].steal();
                i
            }
        };

        self.voices[index].note_on(midi_note, age);
    }

    /// Relâche toutes les voix.
    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.note_off();
        }
    }

    /// Réservé aux tests : sert à vérifier le vol de voix.
    #[cfg(test)]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    pub fn next(&mut self, cutoff_scale: f32) -> f32 {
        self.voices.iter_mut().map(|v| v.next(cutoff_scale)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    #[test]
    fn une_voix_produit_du_son_puis_se_tait() {
        let mut v = Voice::new(FS, ARP);
        v.note_on(69.0, 1);

        let mut energie = 0.0f32;
        for _ in 0..(FS as usize / 10) {
            let s = v.next(1.0);
            assert!(s.is_finite());
            energie += s.abs();
        }
        assert!(energie > 1.0, "la voix n'a rien produit");

        v.note_off();
        for _ in 0..(FS as usize) {
            v.next(1.0);
        }
        assert!(!v.is_active(), "la voix n'a jamais été libérée");
    }

    #[test]
    fn tous_les_preglages_restent_finis_et_bornes() {
        for (nom, preset) in [("pad", PAD), ("bass", BASS), ("arp", ARP), ("lead", LEAD)] {
            let mut v = Voice::new(FS, preset);
            v.note_on(45.0, 1);
            for _ in 0..(FS as usize) {
                let s = v.next(1.0);
                assert!(s.is_finite(), "{nom} produit un non-fini");
                assert!(s.abs() < 8.0, "{nom} déborde : {s}");
            }
        }
    }

    /// Sans vol de voix, un accord joué sur un instrument saturé serait avalé en
    /// silence — le changement d'harmonie deviendrait inaudible.
    #[test]
    fn l_instrument_vole_la_voix_la_plus_ancienne() {
        let mut inst = Instrument::new(FS, PAD, 2);
        inst.note_on(60.0);
        inst.note_on(64.0);
        assert_eq!(inst.active_voices(), 2);

        // Une troisième note ne doit pas être ignorée.
        inst.note_on(67.0);
        assert_eq!(inst.active_voices(), 2);

        let mut energie = 0.0f32;
        for _ in 0..(FS as usize / 4) {
            energie += inst.next(1.0).abs();
        }
        assert!(energie > 1.0, "l'instrument est muet après le vol de voix");
    }

    #[test]
    fn un_instrument_au_repos_est_silencieux() {
        let mut inst = Instrument::new(FS, LEAD, 1);
        for _ in 0..1_000 {
            assert_eq!(inst.next(1.0), 0.0);
        }
    }

    #[test]
    fn le_portamento_glisse_au_lieu_de_sauter() {
        let mut v = Voice::new(FS, LEAD);
        v.note_on(48.0, 1);
        for _ in 0..(FS as usize / 20) {
            v.next(1.0);
        }
        // Nouvelle note pendant que la voix sonne : la fréquence doit glisser.
        v.note_on(72.0, 2);
        let apres_declenchement = v.current_freq;
        v.next(1.0);
        assert!(
            apres_declenchement < midi_to_freq(72.0),
            "la fréquence a sauté au lieu de glisser"
        );
    }
}
