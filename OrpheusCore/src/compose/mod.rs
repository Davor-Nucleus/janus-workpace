//! Décisions musicales : théorie, progressions, arrangement.
//!
//! Séparé de `synth`, qui ne sait produire que du son : ici on décide *quoi*
//! jouer, là-bas *comment* le faire sonner.
//!
//! Tout l'aléa passe par un unique générateur ensemencé, si bien qu'une graine
//! détermine entièrement la musique produite. C'est ce qui rend un rendu
//! reproductible, donc testable, et permet de rejouer une session.

pub mod arranger;
pub mod theory;
