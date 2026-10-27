//! Aviso de actualizaciones: consulta el último release publicado en GitHub.
//!
//! Corre en un hilo aparte que pasa casi todo el tiempo dormido (0 % de CPU): revisa al minuto
//! de abrir la app y luego una vez al día. Si encuentra una versión más nueva, se la manda a la
//! ventana con `PostMessageW`. No envía ningún dato: es un GET sin cookies ni identificadores.

use std::time::Duration;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_APP};

/// Mensaje con la versión nueva (`Box<String>` en lparam).
pub const WM_UPDATE: u32 = WM_APP + 4;

/// Página de descarga que se abre al aceptar el aviso.
pub const DOWNLOAD_URL: &str = "https://github.com/gportal21/notch-isla/releases/latest";

const API_HOST: &str = "api.github.com";
const API_PATH: &str = "/repos/gportal21/notch-isla/releases/latest";

/// Espera antes de la primera consulta, para no competir con el arranque de Windows.
/// En debug son 5 s, para probar el aviso sin esperar.
const FIRST_CHECK: Duration = Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 60 });
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

pub fn start(hwnd: HWND) {
    // HWND no es Send; se pasa como número.
    let hwnd = hwnd.0 as usize;
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        let current = current_version();
        let mut notified: Option<String> = None;
        loop {
            if let Some(version) = latest_version() {
                // Se avisa una sola vez por versión.
                if is_newer(&version, &current)
                    && notified.as_deref() != Some(version.as_str())
                {
                    notified = Some(version.clone());
                    let ptr = Box::into_raw(Box::new(version));
                    let sent = unsafe {
                        PostMessageW(
                            Some(HWND(hwnd as _)),
                            WM_UPDATE,
                            WPARAM(0),
                            LPARAM(ptr as isize),
                        )
                    };
                    if sent.is_err() {
                        // La ventana ya no existe: liberar y terminar.
                        drop(unsafe { Box::from_raw(ptr) });
                        return;
                    }
                }
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// Versión de esta app. En debug se puede fingir otra con la variable NOTCH_ISLA_VERSION
/// (p. ej. "0.1.0") para ver el aviso.
fn current_version() -> String {
    if cfg!(debug_assertions) {
        if let Ok(v) = std::env::var("NOTCH_ISLA_VERSION") {
            return v;
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// Recupera la versión que mandó `start`.
///
/// # Safety
/// `lparam` debe venir de un mensaje `WM_UPDATE`.
pub unsafe fn take(lparam: LPARAM) -> String {
    *unsafe { Box::from_raw(lparam.0 as *mut String) }
}

/// Abre la página de descarga en el navegador.
pub fn open_download_page() {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(DOWNLOAD_URL),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Versión del último release (sin la "v"), o `None` si no hay red o algo falló.
fn latest_version() -> Option<String> {
    let body = fetch_latest_release()?;
    let tag = json_string(&body, "tag_name")?;
    Some(tag.trim_start_matches('v').to_string())
}

fn fetch_latest_release() -> Option<String> {
    unsafe {
        let agent = HSTRING::from(format!("notch-isla/{}", env!("CARGO_PKG_VERSION")));
        let session = Handle(WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            None,
            None,
            0,
        ));
        if session.0.is_null() {
            return None;
        }
        // Resolver, conectar, enviar y recibir: 10 s como máximo cada uno.
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 10_000);

        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(API_HOST), 443, 0));
        if connect.0.is_null() {
            return None;
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            &HSTRING::from(API_PATH),
            None,
            None,
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }
        let headers: Vec<u16> = "Accept: application/vnd.github+json".encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            None,
            Some(&mut status as *mut u32 as *mut _),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;
        if status != 200 {
            return None;
        }

        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        String::from_utf8(body).ok()
    }
}

/// Valor de texto de `"clave": "valor"` en un JSON, sin cargar un parser completo.
fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let after_key = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let value = after_colon.strip_prefix('"')?;
    Some(&value[..value.find('"')?])
}

/// `true` si `candidate` (p. ej. "0.3.0") es mayor que `current`, comparando número a número.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u32>> { v.split('.').map(|n| n.parse().ok()).collect() };
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versiones() {
        assert!(is_newer("0.3.0", "0.2.1"));
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("0.3.0-beta", "0.2.1"));
    }

    /// Usa la red: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn consulta_github() {
        let version = latest_version().expect("sin respuesta de GitHub");
        assert!(is_newer(&version, "0.0.0"), "versión rara: {version}");
    }

    #[test]
    fn lee_tag_name() {
        let json = r#"{"url":"x","tag_name" : "v0.3.0","name":"Notch Isla"}"#;
        assert_eq!(json_string(json, "tag_name"), Some("v0.3.0"));
        assert_eq!(json_string(json, "falta"), None);
    }
}
