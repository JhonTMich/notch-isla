# notch-isla

Isla dinámica (estilo Dynamic Island) para Windows 10/11, escrita en Rust.
Objetivo: controlar la música que suena en el PC (Spotify, YouTube, etc.) desde una píldora en el borde superior de la pantalla, consumiendo lo mínimo posible.

## Estado

| Paso | Qué hace | Estado |
|---|---|---|
| 1 | Píldora negra arriba al centro que se expande con rebote al pasar el mouse | Hecho |
| 2 | Leer la canción que suena (título, artista, carátula) vía SMTC | Hecho |
| 3 | Controles: play/pausa, siguiente, anterior | Hecho |
| 4 | Ícono en la bandeja, inicio con Windows, una sola instancia, ocultar en pantalla completa y sin música | Hecho |

## Requisitos

- Windows 10 (1809+) u 11, 64 bits.
- [Rust](https://rustup.rs) con el toolchain `stable-x86_64-pc-windows-msvc`.
- Visual Studio Build Tools con la carga de trabajo **"Desarrollo para el escritorio con C++"** (aporta el enlazador `link.exe`).

## Ejecutar

```powershell
cargo run            # modo debug (abre una consola para ver mensajes)
cargo run --release  # versión optimizada, sin consola
```

El ejecutable final queda en `target\release\notch-isla.exe`.

### Instalador

`installer\build.ps1` compila la app y genera `target\installer\notch-isla-setup-<versión>.exe` con [Inno Setup 6](https://jrsoftware.org/isinfo.php) (la versión sale de `Cargo.toml`):

```powershell
winget install --id JRSoftware.InnoSetup -e --scope user   # una sola vez
.\installer\build.ps1
```

El instalador no pide permisos de administrador: instala en `%LOCALAPPDATA%\Programs\Notch Isla`, crea el acceso del menú Inicio (y del escritorio, si se elige), ofrece **Iniciar con Windows** y queda en *Aplicaciones instaladas* con su desinstalador, que cierra la isla y quita el inicio automático.

### Ícono

El ícono (`assets/icon.ico`) se dibuja por código y se incrusta en el `.exe` al compilar (`build.rs` + `app.rc`, con el `rc.exe` del Windows SDK). Para regenerarlo después de cambiar el diseño en `examples/icon.rs`:

```powershell
cargo run --example icon
```

**Uso:**

- La isla aparece cuando alguna app reproduce música y se oculta si no hay nada o si una app está a pantalla completa.
- Si no la usas, a los 3 s sube y se oculta para no tapar lo que hay debajo (pestañas del navegador, etc.). Queda una línea fina en el borde superior, con el avance de la canción en el color de la carátula; lleva el mouse a esa línea para traer la isla de vuelta.
- Es una píldora flotante arriba al centro. Contraída muestra una mini carátula y barras de audio (con el color de la carátula) que se mueven mientras suena la música.
- Pasa el mouse sobre la píldora para expandirla: en una sola fila, carátula, título, artista y los botones anterior / play-pausa / siguiente.
- En la esquina de la carátula, el ícono de la app que suena (Spotify, Chrome...). Si hay varias apps con música, unos puntos bajo la carátula lo indican y un clic en ella pasa a la siguiente.
- Debajo, una barra de progreso con el color de la carátula; haz clic en ella para saltar a ese punto de la canción (si la app informa el tiempo).
- Gira la rueda del mouse sobre la isla para subir o bajar el volumen de Windows; la barra muestra el nivel un momento.
- Si el título o el artista no caben, se desplazan de un lado al otro.
- Clic derecho en el ícono de la bandeja: **Iniciar con Windows** y **Salir**.
- Al minuto de abrirse y luego una vez al día, consulta en GitHub si hay una versión nueva. Si la hay, muestra un aviso (clic para ir a la descarga) y agrega **Descargar la versión X** al menú de la bandeja. Ver [PRIVACY.md](PRIVACY.md).

## Cómo está hecha

- **Ventana layered** (`UpdateLayeredWindow`) con transparencia por píxel: los píxeles transparentes dejan pasar los clics a las ventanas de abajo.
- Dibujo con [tiny-skia](https://github.com/linebender/tiny-skia) (anti-aliasing, curvas) sobre un mapa de bits en memoria.
- Animación con un **resorte amortiguado** (`src/spring.rs`). El temporizador de ~60 fps solo existe mientras la isla se mueve: **en reposo, 0 % de CPU**.
- Consciente del DPI por monitor (nítida con escalado 125 %, 150 %…).
- **Música por SMTC** (`Windows.Media.Control`) con eventos, sin sondeo. Los eventos llegan en otros hilos y se pasan a la ventana con `PostMessageW`.
- **Carátula** decodificada con WIC (`Windows.Graphics.Imaging`), recortada al centro.
- **Texto** con DirectWrite, dibujado por Direct2D en modo software sobre el mismo mapa de bits.
- **Pantalla completa** detectada registrando la ventana como appbar (`ABN_FULLSCREENAPP`), también sin sondeo.

```
src/
├─ main.rs        ventana, mensajes de Windows, bucle de animación
├─ island.rs      geometría y dibujo de la isla, botones e ícono de la bandeja
├─ spring.rs      física del resorte
├─ media.rs       música vía SMTC: sesiones, carátula y órdenes de reproducción
├─ appicon.rs     ícono de la app que suena (shell de Windows o su .exe)
├─ text.rs        título y artista con DirectWrite, desplazamiento de textos largos
├─ tray.rs        ícono en la bandeja y su menú
├─ audio.rs       volumen general con Core Audio (rueda del mouse)
├─ autostart.rs   iniciar con Windows (clave Run del usuario)
├─ update.rs      aviso de actualizaciones (último release en GitHub, con WinHTTP)
└─ fullscreen.rs  ocultar la isla cuando una app está a pantalla completa
```

## Compilación en GitHub Actions

`.github/workflows/build.yml` compila el `.exe` y el instalador en cada cambio de `main` y deja los archivos como artefactos de la ejecución. Cuando el repo tenga configurada la firma con SignPath, el mismo flujo firma primero el `.exe`, arma el instalador con él y firma el instalador.

## Licencia

[MIT](LICENSE).
