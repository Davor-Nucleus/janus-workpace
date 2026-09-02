//! Réglages de la console hôte.

/// Renomme la fenêtre de console du processus.
///
/// Sans effet hors Windows, pour que les binaires puissent l'appeler sans `cfg`.
/// Les trois serveurs lancés côte à côte par un même script ouvrent autant de
/// consoles identiques : le titre est le seul moyen de les distinguer.
pub fn set_title(title: &str) {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::iter::once;
        use std::os::windows::ffi::OsStrExt;
        use winapi::um::wincon::SetConsoleTitleW;

        let wide: Vec<u16> = OsStr::new(title).encode_wide().chain(once(0)).collect();
        // SAFETY : `wide` est terminé par un zéro et vit jusqu'à la fin de l'appel.
        unsafe {
            SetConsoleTitleW(wide.as_ptr());
        }
    }

    #[cfg(not(windows))]
    let _ = title;
}
