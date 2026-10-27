//! Incrusta el ícono (`app.rc` → `assets/icon.ico`) en el .exe: se ve en el Explorador, el
//! Administrador de tareas y en "Aplicaciones de arranque". Usa el compilador de recursos del
//! Windows SDK (rc.exe), que viene con las Build Tools de Visual Studio.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("no se pudo compilar app.rc (¿falta el Windows SDK?)");
}
