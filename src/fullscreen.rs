//! Detección de apps a pantalla completa (videos, juegos, presentaciones) sin sondeo.
//!
//! Truco clásico: registrarse como "appbar" sin reservar espacio. El Explorador avisa a todas
//! las appbars con `ABN_FULLSCREENAPP` cuando una app entra o sale de pantalla completa.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_NEW, ABM_REMOVE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, WM_APP};

/// Mensaje con el que el Explorador nos avisa; `wparam` = tipo de aviso.
pub const WM_APPBAR: u32 = WM_APP + 3;
/// `wparam` de "una app entró o salió de pantalla completa" (`lparam` != 0 = entró).
pub const ABN_FULLSCREENAPP: usize = 2;

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APPBAR,
        ..Default::default()
    }
}

pub fn register(hwnd: HWND) {
    let mut d = data(hwnd);
    unsafe {
        SHAppBarMessage(ABM_NEW, &mut d);
    }
}

pub fn unregister(hwnd: HWND) {
    let mut d = data(hwnd);
    unsafe {
        SHAppBarMessage(ABM_REMOVE, &mut d);
    }
}

/// Interpreta el aviso: ¿hay una app a pantalla completa en el monitor de la isla?
pub fn is_active(hwnd: HWND, lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == hwnd {
            return false;
        }
        // Rareza conocida: hacer clic en el escritorio también dispara el aviso.
        let mut class = [0u16; 32];
        let len = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if class == "WorkerW" || class == "Progman" {
            return false;
        }
        // Solo cuenta si es en el mismo monitor que la isla.
        MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
            == MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
    }
}
