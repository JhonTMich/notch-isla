//! Atajo de teclado global para mostrar u ocultar la isla: se puede cambiar o desactivar
//! desde la bandeja.
//!
//! Para cambiarlo, la isla "escucha" la siguiente combinación que se presione (Ctrl, Alt o Win
//! más una tecla) con un gancho de teclado que solo existe mientras dura la espera. Esa
//! combinación no llega a la app que tenga el foco. Esc cancela.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, MapVirtualKeyW, RegisterHotKey, UnregisterHotKey,
    HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN,
};

/// Llega con la combinación capturada: `wparam` = modificadores (MOD_*), `lparam` = tecla
/// (código virtual), o `lparam` = 0 si se canceló con Esc.
pub const WM_HOTKEY_CAPTURED: u32 = WM_APP + 7;
/// Identificador del atajo registrado en Windows.
const ID: i32 = 1;

#[derive(Clone, Copy, PartialEq)]
pub struct Hotkey {
    /// MOD_ALT, MOD_CONTROL, MOD_SHIFT y MOD_WIN combinados.
    pub mods: u32,
    /// Código virtual de la tecla.
    pub vk: u32,
}

/// El de siempre: Ctrl + Alt + N.
pub const DEFAULT: Hotkey = Hotkey {
    mods: MOD_CONTROL.0 | MOD_ALT.0,
    vk: b'N' as u32,
};

impl Hotkey {
    /// Nombre para mostrar: "Ctrl + Alt + N".
    pub fn name(&self) -> String {
        let mut parts = Vec::new();
        for (flag, name) in [
            (MOD_CONTROL.0, "Ctrl"),
            (MOD_ALT.0, "Alt"),
            (MOD_SHIFT.0, "Mayús"),
            (MOD_WIN.0, "Win"),
        ] {
            if self.mods & flag != 0 {
                parts.push(name.to_string());
            }
        }
        parts.push(key_name(self.vk));
        parts.join(" + ")
    }
}

/// Registra el atajo en Windows (antes quita el anterior). Devuelve `false` si otra app ya
/// lo usa; con `None` solo lo quita.
pub fn register(hwnd: HWND, hotkey: Option<Hotkey>) -> bool {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), ID);
        match hotkey {
            Some(h) => RegisterHotKey(
                Some(hwnd),
                ID,
                HOT_KEY_MODIFIERS(h.mods) | MOD_NOREPEAT,
                h.vk,
            )
            .is_ok(),
            None => true,
        }
    }
}

/// Es el mensaje WM_HOTKEY de nuestro atajo.
pub fn is_ours(wparam: WPARAM) -> bool {
    wparam.0 as i32 == ID
}

/// Ventana que recibe la combinación capturada; 0 si no se está capturando.
static CAPTURE_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);

/// Empieza a escuchar la siguiente combinación (llega con `WM_HOTKEY_CAPTURED`).
pub fn start_capture(hwnd: HWND) -> bool {
    stop_capture();
    CAPTURE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) } {
        Ok(hook) => {
            HOOK.store(hook.0 as isize, Ordering::SeqCst);
            true
        }
        Err(_) => {
            CAPTURE_HWND.store(0, Ordering::SeqCst);
            false
        }
    }
}

pub fn stop_capture() {
    CAPTURE_HWND.store(0, Ordering::SeqCst);
    let hook = HOOK.swap(0, Ordering::SeqCst);
    if hook != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(hook as _));
        }
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let target = CAPTURE_HWND.load(Ordering::SeqCst);
    let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    if code >= 0 && down && target != 0 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let vk = info.vkCode;
        let hwnd = Some(HWND(target as _));
        if vk == VK_ESCAPE.0 as u32 {
            CAPTURE_HWND.store(0, Ordering::SeqCst);
            unsafe {
                let _ = PostMessageW(hwnd, WM_HOTKEY_CAPTURED, WPARAM(0), LPARAM(0));
            }
            return LRESULT(1);
        }
        if !is_modifier(vk) {
            let pressed = |k: VIRTUAL_KEY| unsafe { GetAsyncKeyState(k.0 as i32) } < 0;
            let mut mods = 0;
            if pressed(VK_CONTROL) {
                mods |= MOD_CONTROL.0;
            }
            if pressed(VK_MENU) {
                mods |= MOD_ALT.0;
            }
            if pressed(VK_SHIFT) {
                mods |= MOD_SHIFT.0;
            }
            if pressed(VK_LWIN) || pressed(VK_RWIN) {
                mods |= MOD_WIN.0;
            }
            // Solo Mayús no basta: escribiría letras mayúsculas en cualquier app.
            if mods & (MOD_CONTROL.0 | MOD_ALT.0 | MOD_WIN.0) != 0 {
                CAPTURE_HWND.store(0, Ordering::SeqCst);
                unsafe {
                    let _ = PostMessageW(
                        hwnd,
                        WM_HOTKEY_CAPTURED,
                        WPARAM(mods as usize),
                        LPARAM(vk as isize),
                    );
                }
                // La combinación no llega a la app que tiene el foco.
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn is_modifier(vk: u32) -> bool {
    [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|k| k.0 as u32 == vk)
}

/// Nombre de una tecla: letras y números tal cual; el resto, como lo llama Windows
/// ("Espacio", "F5", "Flecha arriba"...) en el idioma del teclado.
fn key_name(vk: u32) -> String {
    if let Some(c) = char::from_u32(vk).filter(|c| c.is_ascii_alphanumeric()) {
        return c.to_string();
    }
    unsafe {
        let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        // Flechas, Inicio, Fin, RePág...: teclas "extendidas" (bit 24), o salen como el teclado
        // numérico.
        let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E);
        let lparam = ((scan as i32) << 16) | if extended { 1 << 24 } else { 0 };
        let mut buf = [0u16; 64];
        let len = GetKeyNameTextW(lparam, &mut buf);
        if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            format!("Tecla {vk}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombre_del_atajo() {
        assert_eq!(DEFAULT.name(), "Ctrl + Alt + N");
        let h = Hotkey {
            mods: MOD_WIN.0 | MOD_SHIFT.0,
            vk: b'7' as u32,
        };
        assert_eq!(h.name(), "Mayús + Win + 7");
    }
}
