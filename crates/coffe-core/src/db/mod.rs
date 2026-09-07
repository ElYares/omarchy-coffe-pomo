//! La capa de persistencia. Un `Db` es una conexión con el esquema al día.

pub mod projects;
pub mod reportes;
pub mod schema;
pub mod tasks;
pub mod timing;

use crate::error::CoffeError;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::path::Path;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Abre (o crea) la base y la deja migrada.
    pub fn open(path: &Path) -> Result<Self, CoffeError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoffeError::Directorio {
                ruta: dir.display().to_string(),
                fuente: e,
            })?;
        }
        Self::desde_conexion(Connection::open(path)?)
    }

    /// Una base en memoria. Es lo que usan las pruebas.
    pub fn en_memoria() -> Result<Self, CoffeError> {
        Self::desde_conexion(Connection::open_in_memory()?)
    }

    fn desde_conexion(conn: Connection) -> Result<Self, CoffeError> {
        // WAL para que la ventana pueda leer mientras el daemon escribe, y
        // claves foráneas porque medio esquema depende de ellas para no dejar
        // pomodoros huérfanos.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Las fechas viven en la base como RFC 3339 en UTC. Un solo formato, y el
/// huso se resuelve al pintar, no al guardar.
pub(crate) fn a_texto(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

pub(crate) fn de_texto(s: &str) -> Result<DateTime<Utc>, CoffeError> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| CoffeError::DatoCorrupto(format!("fecha {s:?}: {e}")))
}

pub(crate) fn de_texto_opt(s: Option<String>) -> Result<Option<DateTime<Utc>>, CoffeError> {
    s.as_deref().map(de_texto).transpose()
}
