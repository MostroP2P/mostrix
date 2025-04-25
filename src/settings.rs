use once_cell::sync::OnceLock;
use serde::Deserialize;
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct Settings {
    mostro_pubkey: String,
    relays:        Vec<String>,
    log_level:     String,
}

// Variable global, solamente se inicializa una vez
static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Construye (o copia) el archivo de configuración y lo carga
fn init_settings() -> &'static Settings {
    SETTINGS.get_or_init(|| {
        // HOME y nombre del paquete en tiempo de compilación
        let home_dir      = env::var("HOME").expect("No se pudo obtener $HOME");
        let package_name  = env!("CARGO_PKG_NAME");          // p.e. "my_project"
        let hidden_dir    = Path::new(&home_dir).join(format!(".{package_name}"));
        let hidden_file   = hidden_dir.join("settings.toml");

        // Ruta del settings.toml incluido en el repo (al lado del Cargo.toml)
        let default_file: PathBuf =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.toml");

        // 1. Crea ~/.my_project si no existe
        if !hidden_dir.exists() {
            fs::create_dir(&hidden_dir)
                .expect("No se pudo crear el directorio de configuración");
        }

        // 2. Copia settings.toml si aún no está en ~/.my_project
        if !hidden_file.exists() {
            fs::copy(&default_file, &hidden_file)
                .expect("No se pudo copiar settings.toml por defecto");
        }

        // 3. Usa el crate `config` para deserializar a la struct Settings
        let cfg = config::Config::builder()
            .add_source(config::File::from(hidden_file))
            .build()
            .expect("settings.toml mal formado");

        cfg.try_deserialize::<Settings>()
            .expect("Error deserializando settings.toml")
    })
}