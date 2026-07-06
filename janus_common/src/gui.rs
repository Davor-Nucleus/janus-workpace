//! Fenêtre Win32 pour afficher des logs en temps réel.
#![cfg(windows)]

use std::ptr::null_mut;
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::time::{Duration, Instant};

use widestring::U16CString;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HWND, RECT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{GetStockObject, WHITE_BRUSH};
use winapi::um::winuser::*;

use crate::logger::get_global_log_buffer_ptr;

pub struct LogWindowHandle;

impl LogWindowHandle {
    pub fn spawn(log_buffer: Arc<Mutex<String>>, on_close: Sender<()>) {
        // Définir le buffer global au démarrage de la fenêtre pour être sûr
        // Note: l'appelant a probablement déjà appelé set_global_log_buffer_ptr
        // Mais on garde une référence ici pour que `spawn` soit le point d'entrée
        let arc_ptr = Arc::into_raw(log_buffer);
        crate::logger::set_global_log_buffer_ptr(arc_ptr);

        // On recrée un Arc pour éviter la fuite mémoire (il sera drop à la fin du thread spawn s'il n'est pas utilisé ailleurs ?)
        // Attention : Arc::into_raw consomme l'Arc. Il faut le reconstituer plus tard ou le garder en vie.
        // Ici, on le passe en pointeur global statique. Le main doit garder son propre Arc ou on accepte que ce soit "leaké" intentionnellement
        // pour la durée de vie de l'app.
        // Mieux : on restaure l'Arc pour le passer au thread, et on set le ptr global juste pour wnd_proc.
        // Pour simplifier : on clone l'Arc avant le spawn, on convertit le clone en raw.

        std::thread::spawn(move || {
            unsafe {
                let hinstance = GetModuleHandleW(std::ptr::null());

                // Nom de classe et titre
                let class_name = U16CString::from_str("JanusCoreLogWndClass").unwrap();
                let window_title = U16CString::from_str("JanusCore - Logs").unwrap();

                let hicon = LoadIconW(hinstance, MAKEINTRESOURCEW(1));

                let wnd_class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: if hicon.is_null() {
                        LoadIconW(null_mut(), IDI_APPLICATION)
                    } else {
                        hicon
                    },
                    hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                    hbrBackground: GetStockObject(WHITE_BRUSH as i32) as HBRUSH,
                    lpszMenuName: null_mut(),
                    lpszClassName: class_name.as_ptr(),
                };
                RegisterClassW(&wnd_class);

                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    window_title.as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    800,
                    600,
                    null_mut(),
                    null_mut(),
                    hinstance,
                    null_mut(),
                );

                // Définir icônes
                let icon_handle = if hicon.is_null() {
                    LoadIconW(null_mut(), IDI_APPLICATION)
                } else {
                    hicon
                };
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon_handle as LPARAM);
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    ICON_SMALL as WPARAM,
                    icon_handle as LPARAM,
                );

                let mut last_invalidate = Instant::now();
                loop {
                    let mut msg = std::mem::MaybeUninit::<MSG>::uninit();
                    while PeekMessageW(msg.as_mut_ptr(), null_mut(), 0, 0, PM_REMOVE) != 0 {
                        let msg = msg.assume_init();
                        if msg.message == WM_QUIT {
                            let _ = on_close.send(());
                            // Nettoyer l'Arc global si on voulait être très propre
                            return;
                        }
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }

                    if last_invalidate.elapsed() >= Duration::from_millis(200) {
                        InvalidateRect(hwnd, null_mut(), 1);
                        last_invalidate = Instant::now();
                    }

                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = std::mem::MaybeUninit::<PAINTSTRUCT>::uninit();
                let hdc = BeginPaint(hwnd, ps.as_mut_ptr());
                let ps = ps.assume_init();
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);

                // Lire le buffer global
                let ptr = get_global_log_buffer_ptr();
                if !ptr.is_null() {
                    // On emprunte le Mutex via le pointeur brut
                    let arc_ref = &*ptr;
                    if let Ok(buf) = arc_ref.lock() {
                        let text = U16CString::from_str(buf.as_str())
                            .unwrap_or_else(|_| U16CString::from_str("").unwrap());
                        DrawTextW(
                            hdc,
                            text.as_ptr(),
                            -1,
                            &mut rect,
                            DT_LEFT | DT_TOP | DT_WORDBREAK,
                        );
                    }
                } else {
                    let text = U16CString::from_str("(Initialisation log...)").unwrap();
                    DrawTextW(
                        hdc,
                        text.as_ptr(),
                        -1,
                        &mut rect,
                        DT_LEFT | DT_TOP | DT_WORDBREAK,
                    );
                }

                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
