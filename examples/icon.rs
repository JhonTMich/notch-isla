//! Genera `assets/icon.ico`, el ícono del ejecutable: una píldora negra con barras de audio
//! sobre un fondo redondeado. Se dibuja con tiny-skia (igual que la isla) en varios tamaños.
//!
//! Para regenerarlo después de cambiar el diseño:
//!
//! ```powershell
//! cargo run --example icon
//! ```

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

/// Tamaños que Windows elige según el lugar (barra de tareas, Explorador, Alt+Tab...).
const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 128];

/// Colores del diseño.
/// Algo más claro que la barra de tareas oscura de Windows (#202020), para que no se pierda.
const BACKGROUND: [u8; 3] = [0x30, 0x30, 0x37];
const PILL: [u8; 3] = [0x0b, 0x0b, 0x0b];
const BARS: [u8; 3] = [0x5d, 0xca, 0xa5];

fn main() {
    let images: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, bmp_entry(&draw(s)))).collect();
    let ico = ico_file(&images);
    std::fs::create_dir_all("assets").expect("no se pudo crear assets/");
    std::fs::write("assets/icon.ico", &ico).expect("no se pudo escribir assets/icon.ico");
    println!(
        "assets/icon.ico: {} tamaños, {} bytes",
        images.len(),
        ico.len()
    );
}

/// Dibuja el ícono en un lienzo de `size` px. El diseño está en una cuadrícula de 96 unidades;
/// en tamaños chicos se usan menos barras y más gruesas para que no se vuelvan borrosas.
fn draw(size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("tamaño inválido");
    let k = size as f32 / 96.0;
    let small = size <= 24;

    fill_rounded(&mut pixmap, BACKGROUND, 4.0, 4.0, 88.0, 88.0, 22.0, k);
    if small {
        // Píldora más grande y tres barras gruesas.
        fill_rounded(&mut pixmap, PILL, 10.0, 28.0, 76.0, 40.0, 20.0, k);
        for (i, h) in [16.0, 28.0, 20.0].into_iter().enumerate() {
            let x = 44.0 + i as f32 * 12.0;
            fill_rounded(&mut pixmap, BARS, x, 48.0 - h / 2.0, 8.0, h, 4.0, k);
        }
    } else {
        fill_rounded(&mut pixmap, PILL, 14.0, 34.0, 68.0, 28.0, 14.0, k);
        for (i, h) in [12.0, 20.0, 10.0, 16.0].into_iter().enumerate() {
            let x = 50.0 + i as f32 * 7.0;
            fill_rounded(&mut pixmap, BARS, x, 48.0 - h / 2.0, 4.0, h, 2.0, k);
        }
    }
    pixmap
}

/// Rectángulo redondeado en unidades de la cuadrícula de 96, escalado por `k`.
#[allow(clippy::too_many_arguments)]
fn fill_rounded(
    pixmap: &mut Pixmap,
    [r, g, b]: [u8; 3],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rad: f32,
    k: f32,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color_rgba8(r, g, b, 255);
    if let Some(path) = rounded_rect(x * k, y * k, w * k, h * k, rad * k) {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    const KAPPA: f32 = 0.552_284_8;
    let r = r.min(w / 2.0).min(h / 2.0);
    let c = r * KAPPA;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + c, y, x1, y + r - c, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + c, x1 - r + c, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - c, y1, x, y1 - r + c, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Una imagen del .ico en formato BMP de 32 bits: cabecera, píxeles BGRA de abajo hacia
/// arriba con alfa sin premultiplicar, y la máscara de 1 bit (en ceros: manda el alfa).
fn bmp_entry(pixmap: &Pixmap) -> Vec<u8> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mask_row = w.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, w, h * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // planos
    out.extend_from_slice(&32u16.to_le_bytes()); // bits por píxel
    out.extend_from_slice(&0u32.to_le_bytes()); // sin compresión
    out.extend_from_slice(&(w * h * 4 + mask_row * h).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolución y paleta: no se usan

    for row in (0..h).rev() {
        for col in 0..w {
            let c = pixmap
                .pixel(col, row)
                .expect("píxel fuera del lienzo")
                .demultiply();
            out.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
        }
    }
    out.resize(out.len() + (mask_row * h) as usize, 0);
    out
}

/// Archivo .ico: cabecera, un índice de 16 bytes por imagen y luego las imágenes.
fn ico_file(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // tipo: ícono
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        // 0 significa 256 en el formato.
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}
