//! Actualizaciones: consulta el último release publicado en GitHub y, si es más nuevo, descarga
//! el instalador para instalarlo con un clic.
//!
//! Corre en un hilo aparte que pasa casi todo el tiempo dormido (0 % de CPU): revisa al minuto
//! de abrir la app y luego una vez al día. No envía ningún dato: son GET sin cookies ni
//! identificadores. El instalador se verifica (tamaño y SHA-256 publicados por GitHub) antes de
//! ofrecerlo. Si la app es la versión portable (no instalada), solo se avisa y se abre la página.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_APP};

use crate::autostart;

/// Mensaje con la actualización encontrada (`Box<Update>` en lparam).
pub const WM_UPDATE: u32 = WM_APP + 4;

/// Página de descarga, para la versión portable o si la descarga falló.
pub const DOWNLOAD_URL: &str = "https://github.com/gportal21/notch-isla/releases/latest";

const API_HOST: &str = "api.github.com";
const API_PATH: &str = "/repos/gportal21/notch-isla/releases/latest";
const DOWNLOAD_HOST: &str = "github.com";
const REPO_PATH: &str = "/gportal21/notch-isla";

/// Entrada del desinstalador que crea Inno Setup (el AppId de installer/notch-isla.iss + "_is1").
const UNINSTALL_KEY: PCWSTR = w!(
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{977B762C-4545-4D26-8CF9-CC072B365CC0}_is1"
);

/// Espera antes de la primera consulta, para no competir con el arranque de Windows.
/// En debug son 5 s, para probar el aviso sin esperar.
const FIRST_CHECK: Duration = Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 60 });
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Versión nueva disponible.
#[derive(Clone)]
pub struct Update {
    pub version: String,
    /// Instalador ya descargado y verificado; `None` si hay que ir a la página de descarga.
    pub installer: Option<PathBuf>,
}

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
            if let Some(update) = check(&current, notified.as_deref()) {
                notified = Some(update.version.clone());
                let ptr = Box::into_raw(Box::new(update));
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
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// Busca una versión más nueva que `current` (y distinta de la ya avisada) y, si la app está
/// instalada, descarga su instalador.
fn check(current: &str, notified: Option<&str>) -> Option<Update> {
    let json = String::from_utf8(get(API_HOST, API_PATH, true)?).ok()?;
    let tag = json_string(&json, "tag_name")?;
    let version = tag.trim_start_matches('v').to_string();
    // Se avisa una sola vez por versión.
    if !is_newer(&version, current) || notified == Some(version.as_str()) {
        return None;
    }
    let installer = if is_installed() {
        download_installer(&json, tag, &version)
    } else {
        None
    };
    Some(Update { version, installer })
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

/// Recupera la actualización que mandó `start`.
///
/// # Safety
/// `lparam` debe venir de un mensaje `WM_UPDATE`.
pub unsafe fn take(lparam: LPARAM) -> Update {
    *unsafe { Box::from_raw(lparam.0 as *mut Update) }
}

impl Update {
    /// Texto de la opción del menú de la bandeja.
    pub fn menu_label(&self) -> String {
        match self.installer {
            Some(_) => format!("Instalar la versión {}", self.version),
            None => format!("Descargar la versión {}", self.version),
        }
    }

    /// Instala si el instalador ya está descargado (devuelve `true`: la app debe cerrarse);
    /// si no, abre la página de descarga.
    pub fn apply(&self) -> bool {
        if let Some(installer) = &self.installer {
            if install(installer) {
                return true;
            }
        }
        open_download_page();
        false
    }
}

/// Abre la página de descarga en el navegador.
fn open_download_page() {
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

/// Ejecuta el instalador sin ventanas. Devuelve `true` si arrancó: entonces la app debe
/// cerrarse, y el instalador la vuelve a abrir al terminar.
fn install(installer: &Path) -> bool {
    // Respeta "Iniciar con Windows" tal como está ahora, aunque se haya cambiado desde la bandeja.
    let tasks = if autostart::is_enabled() {
        "/MERGETASKS=autostart"
    } else {
        "/MERGETASKS=!autostart"
    };
    std::process::Command::new(installer)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", tasks])
        .spawn()
        .is_ok()
}

/// La app corre desde la carpeta donde la dejó el instalador (y no como .exe portable).
fn is_installed() -> bool {
    let mut buf = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            w!("InstallLocation"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    } == ERROR_SUCCESS;
    if !found {
        return false;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let location = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    match (exe_dir, location.canonicalize()) {
        (Some(dir), Ok(location)) => dir.canonicalize().is_ok_and(|dir| dir == location),
        _ => false,
    }
}

/// Descarga el instalador de `version` a la carpeta temporal y lo verifica contra el tamaño y
/// la huella SHA-256 que informa GitHub. Si ya estaba descargado y es correcto, lo reutiliza.
fn download_installer(json: &str, tag: &str, version: &str) -> Option<PathBuf> {
    let name = format!("notch-isla-setup-{version}.exe");
    // Datos del archivo en la lista de "assets": vienen después de su nombre.
    let asset = &json[json.find(&format!("\"{name}\""))?..];
    let size = json_number(asset, "size")?;
    let digest = json_string(asset, "digest")?
        .strip_prefix("sha256:")?
        .to_ascii_lowercase();

    let path = std::env::temp_dir().join(&name);
    let valid = |data: &[u8]| data.len() as u64 == size && sha256_hex(data).as_deref() == Some(digest.as_str());
    if std::fs::read(&path).is_ok_and(|data| valid(&data)) {
        return Some(path);
    }
    // Siempre desde github.com y la ruta del repo; la redirección al almacenamiento de GitHub
    // la sigue WinHTTP.
    let data = get(
        DOWNLOAD_HOST,
        &format!("{REPO_PATH}/releases/download/{tag}/{name}"),
        false,
    )?;
    if !valid(&data) {
        return None;
    }
    std::fs::write(&path, &data).ok()?;
    Some(path)
}

/// Huella SHA-256 en hexadecimal (minúsculas).
fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut hash = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut hash) };
    status
        .is_ok()
        .then(|| hash.iter().map(|b| format!("{b:02x}")).collect())
}

/// GET por HTTPS. `json` = pedir la respuesta de la API de GitHub.
fn get(host: &str, path: &str, json: bool) -> Option<Vec<u8>> {
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
        // Resolver, conectar, enviar y recibir: 30 s como máximo cada uno.
        let _ = WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 30_000);

        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(host), 443, 0));
        if connect.0.is_null() {
            return None;
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            &HSTRING::from(path),
            None,
            None,
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }
        let headers: Vec<u16> = if json {
            "Accept: application/vnd.github+json".encode_utf16().collect()
        } else {
            Vec::new()
        };
        let headers = (!headers.is_empty()).then_some(headers.as_slice());
        WinHttpSendRequest(request.0, headers, None, 0, 0, 0).ok()?;
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
        let mut chunk = vec![0u8; 64 * 1024];
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
        Some(body)
    }
}

/// Lo que sigue a `"clave":` en un JSON (sin espacios delante), sin cargar un parser completo.
fn json_value<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let after_key = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    Some(after_key.trim_start().strip_prefix(':')?.trim_start())
}

/// Valor de texto de `"clave": "valor"`.
fn json_string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let value = json_value(json, key)?.strip_prefix('"')?;
    Some(&value[..value.find('"')?])
}

/// Valor numérico de `"clave": 123`.
fn json_number(json: &str, key: &str) -> Option<u64> {
    let value = json_value(json, key)?;
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    value[..end].parse().ok()
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

    #[test]
    fn lee_json() {
        let json = r#"{"url":"x","tag_name" : "v0.3.0","assets":[{"name":"a.exe","size":10,
            "digest":"sha256:AB"},{"name":"b.exe","size": 2368450,"digest":"sha256:cd"}]}"#;
        assert_eq!(json_string(json, "tag_name"), Some("v0.3.0"));
        assert_eq!(json_string(json, "falta"), None);
        let b = &json[json.find("\"b.exe\"").unwrap()..];
        assert_eq!(json_number(b, "size"), Some(2368450));
        assert_eq!(json_string(b, "digest"), Some("sha256:cd"));
    }

    #[test]
    fn huella_sha256() {
        assert_eq!(
            sha256_hex(b"abc").as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }

    /// Usa la red: `cargo test -- --ignored`. Descarga el instalador del último release y lo
    /// verifica como lo hace la app.
    #[test]
    #[ignore]
    fn consulta_y_descarga_de_github() {
        let json = String::from_utf8(get(API_HOST, API_PATH, true).unwrap()).unwrap();
        let tag = json_string(&json, "tag_name").unwrap();
        let version = tag.trim_start_matches('v');
        assert!(is_newer(version, "0.0.0"), "versión rara: {version}");
        let path = download_installer(&json, tag, version).expect("descarga o verificación fallida");
        assert!(path.exists());
    }
}
