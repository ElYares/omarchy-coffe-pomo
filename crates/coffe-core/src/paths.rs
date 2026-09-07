//! Rutas XDG. Un solo sitio que sepa dónde vive cada cosa.

use std::path::PathBuf;

/// `~/.local/share/coffe/coffe.db`
pub fn database() -> PathBuf {
    data_dir().join("coffe.db")
}

/// `~/.config/coffe/config.toml`
pub fn config() -> PathBuf {
    config_dir().join("config.toml")
}

/// `$XDG_RUNTIME_DIR/coffe.sock`, con `/tmp` como red de seguridad para las
/// sesiones que no traen runtime dir (una consola tty suelta, por ejemplo).
pub fn socket() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("coffe.sock"),
        None => PathBuf::from(format!("/tmp/coffe-{}.sock", users_uid())),
    }
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("coffe")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("coffe")
}

/// El tema activo de Omarchy, de donde salen los colores de la ventana.
pub fn omarchy_colors() -> PathBuf {
    home().join(".config/omarchy/current/theme/colors.toml")
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn users_uid() -> u32 {
    // Sin dependencias de libc: el uid solo se usa para no chocar en /tmp.
    std::fs::read_to_string("/proc/self/loginuid")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1000)
}
