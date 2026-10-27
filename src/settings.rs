//! Ajustes del menú de la bandeja, guardados en el registro del usuario
//! (HKCU\Software\NotchIsla; no requiere permisos de administrador).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ,
};

const KEY: PCWSTR = w!("Software\\NotchIsla");

/// Opciones de "Ocultar sola tras" en segundos; 0 = nunca.
pub const HIDE_DELAYS: [u32; 5] = [3, 5, 10, 30, 0];

#[derive(Clone)]
pub struct Settings {
    /// Segundos sin usarla antes de ocultarse en el borde superior; 0 = no se oculta.
    pub hide_delay: u32,
    /// Pantalla elegida (nombre de dispositivo, como "\\.\DISPLAY2"); vacío = la principal.
    pub monitor: String,
    /// Se esconde mientras hay una app a pantalla completa.
    pub hide_fullscreen: bool,
    /// Avisa al cambiar la salida de audio (audífonos, parlantes).
    pub notices: bool,
    /// Muestra la letra sincronizada (la pide a lrclib.net); apagada si no se activa.
    pub lyrics: bool,
}

impl Settings {
    pub fn load() -> Self {
        Self {
            hide_delay: read_dword(w!("HideDelay")).unwrap_or(3),
            monitor: read_string(w!("Monitor")).unwrap_or_default(),
            hide_fullscreen: read_dword(w!("HideFullscreen")).is_none_or(|v| v != 0),
            notices: read_dword(w!("Notices")).is_none_or(|v| v != 0),
            lyrics: read_dword(w!("Lyrics")).is_some_and(|v| v != 0),
        }
    }

    pub fn save(&self) {
        write_dword(w!("HideDelay"), self.hide_delay);
        write_string(w!("Monitor"), &self.monitor);
        write_dword(w!("HideFullscreen"), self.hide_fullscreen as u32);
        write_dword(w!("Notices"), self.notices as u32);
        write_dword(w!("Lyrics"), self.lyrics as u32);
    }
}

fn read_dword(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

fn read_string(name: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 260];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn write_dword(name: PCWSTR, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            REG_DWORD.0,
            Some((&value as *const u32).cast()),
            std::mem::size_of::<u32>() as u32,
        );
    }
}

fn write_string(name: PCWSTR, value: &str) {
    // Termina en 0 como pide el registro.
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEY,
            name,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        );
    }
}
