//! Configuración de usuario. Los valores por defecto son el canon de Cirillo:
//! 25/5/15 con descanso largo cada 4 pomodoros completados.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub pomodoro: Pomodoro,
    pub agenda: Agenda,
    pub vault: Vault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pomodoro {
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    /// Cada cuántos pomodoros **completados** toca descanso largo.
    pub long_break_every: u32,
    /// En estricto no se puede arrancar un foco durante el descanso ni saltar
    /// el descanso. Apagarlo marca los pomodoros como no canónicos.
    pub strict: bool,
    /// Cirillo: una tarea de más de 7 pomodoros hay que partirla.
    pub max_pomodoros_per_task: u32,
}

/// Con qué se compara la carga de un día.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agenda {
    /// Cuántos pomodoros caben en un día de trabajo. Cirillo habla de ocho a
    /// doce; ocho son cuatro horas de foco, que ya es un día honesto.
    pub pomodoros_por_dia: u32,
    /// Si el sábado y el domingo cuentan como días de trabajo. Por defecto no:
    /// una agenda que da por hecho que trabajas el fin de semana esconde
    /// justo el problema que debería enseñar.
    pub fines_de_semana: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vault {
    /// Raíz del vault de Obsidian. Vacío desactiva el importador.
    pub path: String,
    /// Carpeta de proyectos dentro del vault.
    pub projects_dir: String,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            long_break_every: 4,
            strict: true,
            max_pomodoros_per_task: 7,
        }
    }
}

impl Default for Agenda {
    fn default() -> Self {
        Self { pomodoros_por_dia: 8, fines_de_semana: false }
    }
}

impl Default for Vault {
    fn default() -> Self {
        Self { path: String::new(), projects_dir: "10 Projects".to_string() }
    }
}

impl Config {
    /// La raíz del vault, con el `~` ya resuelto. `None` si no hay vault
    /// configurado o si la ruta no existe.
    ///
    /// El `~` se expande a mano: traerse una dependencia entera por un carácter
    /// no compensa, y es el único que aparece en una ruta escrita a mano.
    pub fn vault_raiz(&self) -> Option<std::path::PathBuf> {
        let bruta = self.vault.path.trim();
        if bruta.is_empty() {
            return None;
        }
        let ruta = match bruta.strip_prefix("~/") {
            Some(resto) => std::path::PathBuf::from(std::env::var("HOME").ok()?).join(resto),
            None => std::path::PathBuf::from(bruta),
        };
        ruta.is_dir().then_some(ruta)
    }

    /// Lee la configuración. Un archivo ausente no es un error: se usan los
    /// valores por defecto, que son los del método clásico.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(texto) => toml::from_str(&texto).map_err(ConfigError::Parse),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Read(e)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no se pudo leer la configuración: {0}")]
    Read(#[source] std::io::Error),
    #[error("la configuración tiene un error de formato: {0}")]
    Parse(#[source] toml::de::Error),
}
