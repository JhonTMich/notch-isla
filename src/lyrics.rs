//! Letra sincronizada de la canción, de LRCLIB (lrclib.net: base pública y gratuita, sin
//! cuenta). Solo se consulta si el usuario activa "Mostrar letra" en la bandeja: se envían
//! título, artista, álbum y duración de la canción.
//!
//! La letra llega en formato LRC, una línea por verso con el momento en que se canta
//! ("[00:12.40] texto"). Se pide una vez por canción, en otro hilo, y el resultado (con o sin
//! letra) se pasa a la ventana con `WM_LYRICS`; la isla lo guarda para no volver a pedirlo.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::update;

/// Mensaje con un `Box<Lyrics>` en `lparam` (se recupera con `take`).
pub const WM_LYRICS: u32 = WM_APP + 6;

const HOST: &str = "lrclib.net";

/// Letra de una canción: versos con el segundo en que empiezan, en orden. Vacía si LRCLIB no
/// la tiene sincronizada (o no se pudo consultar).
pub struct Lyrics {
    /// La canción a la que pertenece (ver `key`).
    pub key: String,
    pub lines: Vec<(f32, String)>,
}

/// Identifica una canción por título y artista.
pub fn key(title: &str, artist: &str) -> String {
    format!("{title}\u{1}{artist}")
}

/// Pide la letra en otro hilo; el resultado llega con `WM_LYRICS`.
pub fn fetch(hwnd: HWND, title: String, artist: String, album: String, duration: Option<f32>) {
    let hwnd = hwnd.0 as isize;
    std::thread::spawn(move || {
        let lines = download(&title, &artist, &album, duration)
            .map(|lrc| parse_lrc(&lrc))
            .unwrap_or_default();
        let result = Box::into_raw(Box::new(Lyrics {
            key: key(&title, &artist),
            lines,
        }));
        unsafe {
            if PostMessageW(
                Some(HWND(hwnd as _)),
                WM_LYRICS,
                WPARAM(0),
                LPARAM(result as isize),
            )
            .is_err()
            {
                drop(Box::from_raw(result));
            }
        }
    });
}

/// Recupera la letra enviada con `WM_LYRICS`.
///
/// # Safety
/// `lparam` debe venir de un mensaje `WM_LYRICS` y usarse una sola vez.
pub unsafe fn take(lparam: LPARAM) -> Lyrics {
    *unsafe { Box::from_raw(lparam.0 as *mut Lyrics) }
}

/// Verso que se está cantando en el segundo `t`, o `None` antes del primero o en una pausa
/// instrumental (verso vacío).
pub fn line_at(lines: &[(f32, String)], t: f32) -> Option<&str> {
    let i = lines.partition_point(|(start, _)| *start <= t);
    let text = lines.get(i.checked_sub(1)?)?.1.as_str();
    (!text.is_empty()).then_some(text)
}

/// Segundo en que empieza el verso siguiente al momento `t`, si queda alguno.
pub fn next_start(lines: &[(f32, String)], t: f32) -> Option<f32> {
    let i = lines.partition_point(|(start, _)| *start <= t);
    lines.get(i).map(|(start, _)| *start)
}

/// Texto LRC de la canción. Primero la búsqueda exacta (título, artista, álbum y duración);
/// si no hay, la búsqueda general por título y artista.
fn download(title: &str, artist: &str, album: &str, duration: Option<f32>) -> Option<String> {
    if let Some(d) = duration {
        let path = format!(
            "/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
            encode(title),
            encode(artist),
            encode(album),
            d.round() as u32
        );
        if let Some(lrc) = get_json(&path).and_then(|json| synced(&json)) {
            return Some(lrc);
        }
    }
    let path = format!(
        "/api/search?track_name={}&artist_name={}",
        encode(title),
        encode(artist)
    );
    get_json(&path).and_then(|json| synced(&json))
}

fn get_json(path: &str) -> Option<String> {
    String::from_utf8(update::get(HOST, path, false)?).ok()
}

/// El primer `"syncedLyrics"` con texto del JSON (en la búsqueda llegan varios resultados y
/// algunos no tienen letra sincronizada: vienen con `null`).
fn synced(json: &str) -> Option<String> {
    const KEY: &str = "\"syncedLyrics\"";
    let mut rest = json;
    while let Some(at) = rest.find(KEY) {
        rest = &rest[at + KEY.len()..];
        let value = rest.trim_start().strip_prefix(':')?.trim_start();
        if let Some(text) = value.strip_prefix('"') {
            let lrc = json_unescape(text)?;
            if !lrc.trim().is_empty() {
                return Some(lrc);
            }
        }
    }
    None
}

/// Lee una cadena JSON (a partir de su comilla de apertura, ya quitada) hasta la comilla de
/// cierre, resolviendo los escapes (\n, \", \uXXXX...).
fn json_unescape(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' | 'f' => {}
                'u' => {
                    let hi = hex4(&mut chars)?;
                    let code = if (0xD800..0xDC00).contains(&hi) {
                        // Par sustituto (caracteres fuera del plano básico de Unicode).
                        if chars.next()? != '\\' || chars.next()? != 'u' {
                            return None;
                        }
                        let lo = hex4(&mut chars)?;
                        0x10000 + ((hi - 0xD800) << 10) + (lo.checked_sub(0xDC00)?)
                    } else {
                        hi
                    };
                    out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Los 4 dígitos hexadecimales de un escape \uXXXX.
fn hex4(chars: &mut std::str::Chars) -> Option<u32> {
    let hex: String = chars.take(4).collect();
    u32::from_str_radix(&hex, 16).ok()
}

/// Versos de un texto LRC, en orden. Una línea puede llevar varias marcas de tiempo
/// ("[00:12.40][01:30.10] estribillo"); las de datos ("[ar:Artista]") se ignoran.
fn parse_lrc(lrc: &str) -> Vec<(f32, String)> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some(end) = tag.find(']') else { break };
            match parse_time(&tag[..end]) {
                Some(t) => times.push(t),
                None => break,
            }
            rest = tag[end + 1..].trim_start();
        }
        let text = rest.trim().to_string();
        for t in times {
            lines.push((t, text.clone()));
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

/// "mm:ss.xx" -> segundos.
fn parse_time(tag: &str) -> Option<f32> {
    let (min, sec) = tag.split_once(':')?;
    let min: f32 = min.trim().parse().ok()?;
    let sec: f32 = sec.trim().parse().ok()?;
    Some(min * 60.0 + sec)
}

/// Codifica un texto para la dirección (UTF-8 con %XX, salvo letras, números y -_.~).
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_lrc() {
        let lines =
            parse_lrc("[ar:Alguien]\n[00:12.40] Uno\n[00:05.00][01:00.50]Coro\n[00:20.00]\n");
        assert_eq!(
            lines,
            vec![
                (5.0, "Coro".to_string()),
                (12.4, "Uno".to_string()),
                (20.0, String::new()),
                (60.5, "Coro".to_string()),
            ]
        );
        assert_eq!(line_at(&lines, 1.0), None);
        assert_eq!(line_at(&lines, 5.0), Some("Coro"));
        assert_eq!(line_at(&lines, 15.0), Some("Uno"));
        // Pausa instrumental.
        assert_eq!(line_at(&lines, 30.0), None);
        assert_eq!(line_at(&lines, 99.0), Some("Coro"));
    }

    #[test]
    fn lee_json_de_lrclib() {
        let json = r#"[{"id":1,"syncedLyrics":null},{"id":2,"syncedLyrics":"[00:01.00] Corazón \"bello\"\n[00:02.00] 𝄞"}]"#;
        assert_eq!(
            synced(json).as_deref(),
            Some("[00:01.00] Corazón \"bello\"\n[00:02.00] \u{1D11E}")
        );
        assert_eq!(synced(r#"{"syncedLyrics":null}"#), None);
    }

    #[test]
    fn codifica_la_direccion() {
        assert_eq!(encode("Canción & Co"), "Canci%C3%B3n%20%26%20Co");
    }

    /// Consulta real a lrclib.net (se ignora por defecto: necesita internet).
    #[test]
    #[ignore]
    fn consulta_lrclib() {
        let lrc = download(
            "Bohemian Rhapsody",
            "Queen",
            "A Night at the Opera",
            Some(355.0),
        )
        .expect("sin letra");
        let lines = parse_lrc(&lrc);
        println!("{} versos, primero: {:?}", lines.len(), lines.first());
        assert!(lines.len() > 10);
    }
}
