//! Briques de diffusion audio HTTP.
//!
//! Les serveurs historiques du workspace (JanusCore, PhonosCore) poussent leurs
//! échantillons dans un `rodio::Sink`, donc c'est la carte son qui cadence la
//! lecture : le `Sink` ne consomme la source qu'au rythme du périphérique.
//!
//! Sans périphérique, ce garde-fou disparaît — un `Decoder` lu en boucle serrée
//! avale un fichier de quatre minutes en une fraction de seconde. Ce module
//! fournit les trois pièces qui remplacent la carte son :
//!
//! - [`Pacer`] : rend le temps réel au producteur ;
//! - [`Mp3Encoder`] : encapsule LAME, qui est figé à un format à la construction ;
//! - [`StreamHub`] : diffuse les octets encodés vers N auditeurs HTTP.
//!
//! Le choix du MP3 simplifie radicalement le dernier point : ses trames sont
//! auto-délimitées, donc un client qui se connecte en cours de route — ou qui
//! perd des chunks — se resynchronise seul sur l'en-tête de trame suivant. Il n'y
//! a ni en-tête de flux à rejouer, ni reprise à négocier.

pub mod encoder;
pub mod hub;
pub mod pacer;

pub use encoder::Mp3Encoder;
pub use hub::{StreamHub, Subscription};
pub use pacer::Pacer;

/// Fréquence d'échantillonnage du flux encodé.
///
/// LAME est verrouillé sur une fréquence et un nombre de canaux au moment du
/// `build()`, alors que la bibliothèque musicale mélange 44,1 kHz et 48 kHz, mono
/// et stéréo. Tout est donc ramené à ce format unique avant l'encodage.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

/// Nombre de canaux du flux encodé.
pub const OUTPUT_CHANNELS: u16 = 2;

/// Frames produites par itération du moteur.
///
/// Multiple de 1152 — la taille d'une trame MP3 — pour que chaque chunk publié
/// contienne un nombre entier de trames. 8 trames ≈ 192 ms à 48 kHz : assez court
/// pour que le démarrage soit vif, assez long pour que le coût par chunk (verrou,
/// appel LAME, diffusion) reste négligeable.
pub const CHUNK_FRAMES: usize = 1152 * 8;

/// Échantillons entrelacés par chunk (frames × canaux).
pub const CHUNK_SAMPLES: usize = CHUNK_FRAMES * OUTPUT_CHANNELS as usize;
