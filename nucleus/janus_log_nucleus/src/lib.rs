//! Logger centralisé : envoie les logs vers la console et la fenêtre GUI.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;

// Buffer global partagé avec la fenêtre de logs Win32. Les tâches tokio et le thread
// GUI y accèdent en parallèle : des `static mut` nus constituaient un accès concurrent
// non synchronisé (comportement indéfini). Les primitives atomiques rendent l'accès
// correct et permettront de passer janus_nucleus en edition 2024.
static LOG_BUFFER_PTR: AtomicPtr<Mutex<String>> = AtomicPtr::new(std::ptr::null_mut());
static GUI_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_gui_enabled(enabled: bool) {
    GUI_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Enregistre le pointeur vers le buffer de logs.
///
/// L'appelant doit fournir un pointeur issu d'un `Arc` **volontairement fuité**
/// (`Arc::into_raw` sans `from_raw` correspondant), afin qu'il reste valide pendant
/// toute la vie du processus.
pub fn set_global_log_buffer_ptr(ptr: *const Mutex<String>) {
    LOG_BUFFER_PTR.store(ptr as *mut Mutex<String>, Ordering::Release);
}

pub fn get_global_log_buffer_ptr() -> *const Mutex<String> {
    LOG_BUFFER_PTR.load(Ordering::Acquire)
}

pub fn log_to_buffer(msg: &str) {
    let ptr = LOG_BUFFER_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }

    // SAFETY : le pointeur vient d'un Arc fuité par `gui::LogWindowHandle::spawn`,
    // il est donc valide jusqu'à la fin du processus, et le `Mutex` sérialise l'accès
    // au contenu.
    let buffer = unsafe { &*ptr };
    if let Ok(mut guard) = buffer.lock() {
        guard.push_str(msg);
        guard.push('\n');
    }
}

#[inline]
pub fn log_info<S: AsRef<str>>(s: S) {
    let msg = format!("[INFO] {}", s.as_ref());
    println!("{}", msg);
    if GUI_ENABLED.load(Ordering::Relaxed) {
        log_to_buffer(&msg);
    }
}

#[inline]
pub fn log_error<S: AsRef<str>>(s: S) {
    let msg = format!("[ERROR] {}", s.as_ref());
    eprintln!("{}", msg);
    if GUI_ENABLED.load(Ordering::Relaxed) {
        log_to_buffer(&msg);
    }
}
