//! Los colores de Omarchy, leídos del tema activo.
//!
//! La ventana no trae paleta propia: la toma de
//! `~/.config/omarchy/current/theme/colors.toml`, que es el mismo archivo que
//! usan la terminal, btop y el resto del escritorio. Así cambiar de tema cambia
//! también la ventana, sin configurar nada.
//!
//! **La taza no entra en esto.** Su borde es blanco y su café es café; si
//! siguiera al tema, un tema verde daría café verde.

use coffe_core::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Lo que el frontend convierte en variables CSS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tema {
    pub nombre: String,
    pub accent: String,
    pub foreground: String,
    pub background: String,
    pub selection_background: String,
    /// `color0`..`color15` tal cual, por si hace falta un tono concreto.
    pub colores: Vec<String>,
    /// Si el fondo es oscuro. Decide los contrastes de la interfaz.
    pub oscuro: bool,
}

impl Default for Tema {
    /// Si no hay Omarchy debajo --otra distro, o el archivo aún no existe--
    /// la ventana tiene que abrir igual. Estos son los tonos de un café.
    fn default() -> Self {
        Self {
            nombre: "coffe".into(),
            accent: "#cbb693".into(),
            foreground: "#e6e1da".into(),
            background: "#1a1512".into(),
            selection_background: "#3a2f28".into(),
            colores: Vec::new(),
            oscuro: true,
        }
    }
}

pub fn leer() -> Tema {
    let ruta = paths::omarchy_colors();
    let Ok(texto) = std::fs::read_to_string(&ruta) else {
        return Tema::default();
    };

    let mut t = Tema { nombre: nombre_del_tema(), ..Tema::default() };
    let mut colores: Vec<(usize, String)> = Vec::new();

    // El archivo es TOML plano de `clave = "#rrggbb"`. Se lee a mano para no
    // arrastrar el parser entero por seis claves, y para que una clave nueva de
    // Omarchy no rompa nada: lo que no se reconoce se ignora.
    for linea in texto.lines() {
        let Some((clave, valor)) = linea.split_once('=') else { continue };
        let clave = clave.trim();
        let valor = valor.trim().trim_matches('"').trim().to_string();
        if !valor.starts_with('#') {
            continue;
        }

        match clave {
            "accent" => t.accent = valor,
            "foreground" => t.foreground = valor,
            "background" => t.background = valor,
            "selection_background" => t.selection_background = valor,
            otra => {
                if let Some(n) = otra.strip_prefix("color").and_then(|n| n.parse().ok()) {
                    colores.push((n, valor));
                }
            }
        }
    }

    colores.sort_by_key(|(n, _)| *n);
    t.colores = colores.into_iter().map(|(_, c)| c).collect();
    t.oscuro = es_oscuro(&t.background);
    t
}

fn nombre_del_tema() -> String {
    std::fs::read_to_string(ruta_nombre())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "desconocido".into())
}

pub fn ruta_nombre() -> PathBuf {
    paths::omarchy_colors()
        .parent()
        .and_then(|p| p.parent())
        .map_or_else(|| PathBuf::from("/nonexistent"), |p| p.join("theme.name"))
}

/// Luminancia percibida. Se usa para decidir si la interfaz tira a claro o a
/// oscuro, no para pintar nada.
fn es_oscuro(hex: &str) -> bool {
    let Some((r, g, b)) = rgb(hex) else { return true };
    // Coeficientes de luma de Rec. 601: el ojo ve el verde mucho más que el azul.
    (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) < 128.0
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn un_fondo_claro_no_se_toma_por_oscuro() {
        assert!(es_oscuro("#161a20"));
        assert!(es_oscuro("#000000"));
        assert!(!es_oscuro("#ffffff"));
        assert!(!es_oscuro("#ece0d1"));
    }

    #[test]
    fn un_hex_roto_no_tumba_la_ventana() {
        assert_eq!(rgb("#zzzzzz"), None);
        assert_eq!(rgb("#fff"), None);
        assert!(es_oscuro("basura"), "ante la duda, oscuro, que es lo normal");
    }
}
