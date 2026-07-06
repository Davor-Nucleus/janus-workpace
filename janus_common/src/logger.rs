//! Logger centralisé : envoie les logs vers la console et la fenêtre GUI.

use std::sync::Mutex;

// Utilisation d'un buffer global pour simplifier l'accès depuis la GUI Win32
static mut LOG_BUFFER_PTR: *const Mutex<String> = std::ptr::null();
static mut GUI_ENABLED: bool = true;

pub fn set_gui_enabled(enabled: bool) {
    unsafe {
        GUI_ENABLED = enabled;
    }
}

/// Permet à l'application principale de définir le pointeur brut vers le buffer de logs
/// (géré par un Arc<Mutex<String>> créé dans main)
pub fn set_global_log_buffer_ptr(ptr: *const Mutex<String>) {
    unsafe {
        LOG_BUFFER_PTR = ptr;
    }
}

pub fn get_global_log_buffer_ptr() -> *const Mutex<String> {
    unsafe { LOG_BUFFER_PTR }
}

// Fonction utilitaire pour écrire dans le buffer global (safe wrapper)
pub fn log_to_buffer(msg: &str) {
    unsafe {
        if !LOG_BUFFER_PTR.is_null() {
            // Recréer temporairement l'Arc pour verrouiller, MAIS SANS le drop
            // Pour être sûr, on utilise from_raw et on oublie (mem::forget) ou on manie le pointeur
            // Plus simple : on accède via le raw pointer si possible, mais Mutex methods prennent &self.
            // La méthode "sale" mais courante en FFI/legacy :
            let arc_ref = &*LOG_BUFFER_PTR; // Déréférencement pointeur brut -> référence
            if let Ok(mut guard) = arc_ref.lock() {
                guard.push_str(msg);
                guard.push('\n');
            }
        }
    }
}

#[inline]
pub fn log_info<S: AsRef<str>>(s: S) {
    let msg = format!("[INFO] {}", s.as_ref());
    println!("{}", msg);
    unsafe {
        if GUI_ENABLED {
            log_to_buffer(&msg);
        }
    }
}

#[inline]
pub fn log_error<S: AsRef<str>>(s: S) {
    let msg = format!("[ERROR] {}", s.as_ref());
    eprintln!("{}", msg);
    unsafe {
        if GUI_ENABLED {
            log_to_buffer(&msg);
        }
    }
}
