//! Leer las historias de usuario del vault de Obsidian.
//!
//! El vault manda sobre QUÉ hay que hacer; coffe manda sobre el tiempo. Por eso
//! esto solo lee: no escribe en el vault ni pretende sincronizar en dos
//! direcciones.
//!
//! El formato real es más sucio que la convención escrita —diez estados
//! distintos, notas sin prioridad, tres maneras de escribir el tipo— así que
//! aquí se tolera todo y **lo que no se reconoce se dice en voz alta** en vez de
//! adivinarlo en silencio.

use crate::model::Priority;
use serde::{Deserialize, Serialize};

/// Una historia de usuario o caso de uso del Backlog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nota {
    /// `HU-001`, `CU-014`.
    pub id: String,
    /// Sin el prefijo del proyecto: `HU-001 - Cooldown de alertas`.
    pub titulo: String,
    /// El `status` tal como está escrito, sin normalizar.
    pub estado: String,
    /// `None` cuando la nota no trae `prioridad`.
    pub prioridad: Option<Priority>,
    /// Ya no hay nada que hacer con ella.
    pub terminal: bool,
    /// Si el estado es uno de los que sabemos interpretar.
    pub estado_conocido: bool,
    /// Estimación en pomodoros, si la nota la trae. La convención del vault no
    /// la tiene; se lee por si alguien la añade.
    pub pomodoros: Option<u32>,
    /// Fecha de entrega, `AAAA-MM-DD`, si la nota la trae.
    pub entrega: Option<String>,
}

/// Estados en los que la historia ya no es trabajo pendiente.
const TERMINALES: &[&str] = &["hecha", "descartada", "implementada", "cerrado", "cerrada"];

/// Estados que sabemos leer aunque sigan vivos. Los que no estén ni aquí ni en
/// `TERMINALES` se importan como pendientes **y se reportan**: dar por hecha una
/// historia por no reconocer su estado sería la peor equivocación posible.
const VIVOS: &[&str] = &[
    "propuesta",
    "lista",
    "en curso",
    "en-curso",
    "en progreso",
    "en-progreso",
    "en-pruebas",
    "bloqueado",
    "bloqueada",
];

pub fn es_terminal(estado: &str) -> bool {
    TERMINALES.contains(&estado.trim().to_lowercase().as_str())
}

pub fn estado_conocido(estado: &str) -> bool {
    let e = estado.trim().to_lowercase();
    TERMINALES.contains(&e.as_str()) || VIVOS.contains(&e.as_str())
}

/// Lee una nota. `nombre` es el del archivo, con o sin `.md`.
///
/// Devuelve `None` si no parece una historia: sin frontmatter, o sin `id`. No es
/// un error — en un Backlog puede haber un índice o un borrador.
pub fn parsear(nombre: &str, contenido: &str) -> Option<Nota> {
    let fm = frontmatter(contenido)?;
    let id = valor(&fm, "id")?;

    let estado = valor(&fm, "status").unwrap_or_else(|| "propuesta".into());
    let prioridad = valor(&fm, "prioridad").and_then(|p| p.parse().ok());

    Some(Nota {
        titulo: titulo_desde_nombre(nombre, &id),
        terminal: es_terminal(&estado),
        estado_conocido: estado_conocido(&estado),
        pomodoros: valor(&fm, "pomodoros").and_then(|p| p.parse().ok()),
        entrega: valor(&fm, "entrega").filter(|e| e.len() == 10),
        id,
        estado,
        prioridad,
    })
}

/// El bloque entre los dos `---` del principio.
fn frontmatter(contenido: &str) -> Option<Vec<(String, String)>> {
    let resto = contenido.strip_prefix("---")?;
    let fin = resto.find("\n---")?;

    Some(
        resto[..fin]
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| {
                (k.trim().to_lowercase(), v.trim().trim_matches('"').trim_matches('\'').to_string())
            })
            .collect(),
    )
}

fn valor(fm: &[(String, String)], clave: &str) -> Option<String> {
    fm.iter().find(|(k, _)| k == clave).map(|(_, v)| v.clone()).filter(|v| !v.is_empty())
}

/// `devherd - HU-001 - Cooldown de alertas.md` → `HU-001 - Cooldown de alertas`.
///
/// El nombre del proyecto sobra: en coffe la tarea ya cuelga de su proyecto, y
/// repetirlo en cada tarjeta gasta el ancho que necesita el título.
fn titulo_desde_nombre(nombre: &str, id: &str) -> String {
    let base = nombre.strip_suffix(".md").unwrap_or(nombre).trim();

    // Se corta por el ID y no por la primera raya: un proyecto con guiones en
    // el nombre --`tl-mas-server`-- partiría por donde no toca.
    if let Some(pos) = base.find(id) {
        let desde_id = base[pos..].trim();
        if !desde_id.is_empty() {
            return desde_id.to_string();
        }
    }
    base.to_string()
}

/// Lo que hay que hacer con una nota al importarla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destino {
    /// Trabajo pendiente: entra al tablero.
    Importar,
    /// Ya está hecha o descartada: no es trabajo.
    Omitir,
}

pub fn destino(nota: &Nota, incluir_terminadas: bool) -> Destino {
    if nota.terminal && !incluir_terminadas { Destino::Omitir } else { Destino::Importar }
}

/// La prioridad con la que entra una nota sin `prioridad` escrita.
///
/// Media y no alta: si todo lo que llega sin etiqueta fuera urgente, el tablero
/// entero sería urgente y la prioridad dejaría de ordenar nada.
pub const PRIORIDAD_POR_DEFECTO: Priority = Priority::Medium;

// ------------------------------------------------------------------ disco

use std::path::{Path, PathBuf};

/// Una nota encontrada, con su ruta relativa al vault para poder volver a ella.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotaHallada {
    pub nota: Nota,
    /// Relativa a la raíz del vault: `10 Projects/devherd/Backlog/....md`.
    pub ruta: String,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("no encuentro la carpeta {0}")]
    SinCarpeta(String),
    #[error("no pude leer {ruta}: {fuente}")]
    Lectura {
        ruta: String,
        #[source]
        fuente: std::io::Error,
    },
}

/// Las carpetas de proyecto que tienen Backlog, por nombre.
pub fn proyectos_con_backlog(raiz: &Path, dir_proyectos: &str) -> Result<Vec<String>, VaultError> {
    let base = raiz.join(dir_proyectos);
    let entradas = std::fs::read_dir(&base)
        .map_err(|e| VaultError::Lectura { ruta: base.display().to_string(), fuente: e })?;

    let mut nombres: Vec<String> = entradas
        .flatten()
        .filter(|e| e.path().join("Backlog").is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    nombres.sort();
    Ok(nombres)
}

/// Lee el Backlog de una carpeta de proyecto del vault.
///
/// Lo que no parece una historia se ignora sin ruido: en un Backlog puede haber
/// un índice, una plantilla o un borrador.
pub fn leer_backlog(
    raiz: &Path,
    dir_proyectos: &str,
    carpeta: &str,
) -> Result<Vec<NotaHallada>, VaultError> {
    let dir: PathBuf = raiz.join(dir_proyectos).join(carpeta).join("Backlog");
    if !dir.is_dir() {
        return Err(VaultError::SinCarpeta(dir.display().to_string()));
    }

    let mut halladas = Vec::new();
    let entradas = std::fs::read_dir(&dir)
        .map_err(|e| VaultError::Lectura { ruta: dir.display().to_string(), fuente: e })?;

    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let Some(nombre) = ruta.file_name().and_then(|n| n.to_str()) else { continue };
        // Un archivo ilegible no puede tumbar la importación entera.
        let Ok(texto) = std::fs::read_to_string(&ruta) else { continue };

        if let Some(nota) = parsear(nombre, &texto) {
            halladas.push(NotaHallada {
                nota,
                ruta: format!("{dir_proyectos}/{carpeta}/Backlog/{nombre}"),
            });
        }
    }

    halladas.sort_by(|a, b| a.nota.id.cmp(&b.nota.id));
    Ok(halladas)
}

// ------------------------------------------------------------ leer el cuerpo

/// El cuerpo de una nota, ya sin frontmatter.
///
/// El frontmatter es para la máquina —`status`, `id`, `pomodoros`— y ya se leyó
/// al importar. Enseñárselo al usuario sería repetir en crudo lo que la tarjeta
/// ya pinta bonito.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuerpo {
    /// Markdown tal cual, sin el bloque `---`.
    pub texto: String,
    /// La ruta relativa, para poder decir de dónde salió esto.
    pub ruta: String,
}

/// Lee una nota del vault a partir de su ruta **relativa** a la raíz.
///
/// La ruta viene de la base, escrita por el importador, pero se comprueba igual
/// que no se salga del vault: una nota con `../../..` dentro convertiría un
/// visor de notas en un lector de cualquier archivo del disco. Es barato
/// comprobarlo y el día que alguien edite esa columna a mano ya está hecho.
pub fn leer_nota(raiz: &Path, relativa: &str) -> Result<Cuerpo, VaultError> {
    if relativa.is_empty() {
        return Err(VaultError::SinCarpeta(String::from("(ruta vacía)")));
    }

    let completa = raiz.join(relativa);
    // `canonicalize` resuelve `..` y los enlaces, que es justo lo que hay que
    // resolver ANTES de comparar: comparar las cadenas sin resolver deja pasar
    // `Backlog/../../../.ssh/id_rsa`.
    let (Ok(real), Ok(base)) = (completa.canonicalize(), raiz.canonicalize()) else {
        return Err(VaultError::SinCarpeta(completa.display().to_string()));
    };
    if !real.starts_with(&base) {
        return Err(VaultError::SinCarpeta(format!("{relativa} se sale del vault")));
    }

    let texto = std::fs::read_to_string(&real)
        .map_err(|e| VaultError::Lectura { ruta: real.display().to_string(), fuente: e })?;

    Ok(Cuerpo { texto: sin_frontmatter(&texto).to_string(), ruta: relativa.to_string() })
}

/// Quita el bloque `---` de cabecera. Si no lo hay, devuelve todo: una nota sin
/// frontmatter sigue siendo una nota que se puede leer.
fn sin_frontmatter(contenido: &str) -> &str {
    let Some(resto) = contenido.strip_prefix("---") else { return contenido };
    let Some(fin) = resto.find("\n---") else { return contenido };
    resto[fin + 4..].trim_start_matches(['\r', '\n'])
}
