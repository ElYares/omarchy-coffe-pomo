//! Las tareas y sus reglas de estado.

use super::{Db, a_texto, de_texto, de_texto_opt};
use crate::error::CoffeError;
use crate::model::{Priority, TaskState};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub notes: Option<String>,
    pub priority: Priority,
    pub state: TaskState,
    pub estimate_pomodoros: Option<u32>,
    /// `YYYY-MM-DD` en hora local: una entrega es un día, no un instante.
    pub due_date: Option<String>,
    pub vault_note: Option<String>,
    pub position: i64,
    pub created_at: DateTime<Utc>,
    pub first_started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NuevaTarea {
    pub project_id: i64,
    pub title: String,
    pub notes: Option<String>,
    pub priority: Priority,
    pub estimate_pomodoros: Option<u32>,
    pub due_date: Option<String>,
    pub vault_note: Option<String>,
}

/// Lo que Cirillo diría de una estimación. No bloquea nada: avisa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "aviso", rename_all = "snake_case")]
pub enum AvisoEstimacion {
    /// Más de `max` pomodoros: hay que partir la tarea.
    DemasiadoGrande { estimados: u32, max: u32 },
    /// Menos de uno: júntala con otra pequeña.
    DemasiadoChica,
}

pub fn revisar_estimacion(estimados: Option<u32>, max: u32) -> Option<AvisoEstimacion> {
    match estimados {
        Some(0) => Some(AvisoEstimacion::DemasiadoChica),
        Some(n) if n > max => Some(AvisoEstimacion::DemasiadoGrande { estimados: n, max }),
        _ => None,
    }
}

/// Qué tareas se quieren. Todo `None` es "todas las vivas".
#[derive(Debug, Clone, Default)]
pub struct FiltroTareas {
    pub project_id: Option<i64>,
    /// Incluye los subproyectos del `project_id` dado.
    pub incluir_descendientes: bool,
    pub states: Option<Vec<TaskState>>,
    pub priority: Option<Priority>,
}

impl Db {
    pub fn crear_tarea(&self, nueva: NuevaTarea, now: DateTime<Utc>) -> Result<i64, CoffeError> {
        // Al final de la columna de pendientes.
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM tasks WHERE project_id = ?1",
            params![nueva.project_id],
            |f| f.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO tasks
               (project_id, title, notes, priority, state, estimate_pomodoros,
                due_date, vault_note, position, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9)",
            params![
                nueva.project_id,
                nueva.title,
                nueva.notes,
                nueva.priority.as_str(),
                nueva.estimate_pomodoros,
                nueva.due_date,
                nueva.vault_note,
                position,
                a_texto(now),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn tarea(&self, id: i64) -> Result<Task, CoffeError> {
        self.conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id], fila_a_tarea)
            .optional()?
            .ok_or(CoffeError::NoExiste { que: "tarea", id })?
    }

    pub fn tareas(&self, filtro: &FiltroTareas) -> Result<Vec<Task>, CoffeError> {
        // Se arma a mano porque el número de estados es variable y rusqlite no
        // expande listas. Los valores siguen yendo ligados, nunca interpolados.
        let mut sql = String::from("SELECT t.* FROM tasks t WHERE 1 = 1");
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(pid) = filtro.project_id {
            if filtro.incluir_descendientes {
                sql.push_str(
                    " AND t.project_id IN (
                          WITH RECURSIVE bajada(id) AS (
                              SELECT id FROM projects WHERE id = ?
                              UNION ALL
                              SELECT p.id FROM projects p JOIN bajada b ON p.parent_id = b.id
                          ) SELECT id FROM bajada)",
                );
            } else {
                sql.push_str(" AND t.project_id = ?");
            }
            vals.push(Box::new(pid));
        }

        match &filtro.states {
            Some(estados) if !estados.is_empty() => {
                sql.push_str(" AND t.state IN (");
                for (i, e) in estados.iter().enumerate() {
                    if i > 0 {
                        sql.push(',');
                    }
                    sql.push('?');
                    vals.push(Box::new(e.as_str()));
                }
                sql.push(')');
            }
            // Sin filtro explícito, lo archivado no estorba.
            _ => sql.push_str(" AND t.state <> 'archived'"),
        }

        if let Some(p) = filtro.priority {
            sql.push_str(" AND t.priority = ?");
            vals.push(Box::new(p.as_str()));
        }

        // Primero lo que vence antes; a igual fecha, lo más prioritario.
        sql.push_str(
            " ORDER BY t.due_date IS NULL, t.due_date,
                       CASE t.priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
                       t.position",
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|b| b.as_ref()).collect();
        let filas = stmt.query_map(refs.as_slice(), fila_a_tarea)?;
        filas.collect::<Result<Result<Vec<_>, _>, _>>()?
    }

    /// Solo mientras la tarea siga sin arrancar. En cuanto tiene un pomodoro
    /// encima, la prioridad con la que se trabajó es historia y no se reescribe.
    pub fn cambiar_prioridad(&self, id: i64, prioridad: Priority) -> Result<(), CoffeError> {
        let tarea = self.tarea(id)?;
        if tarea.state.ya_arranco() {
            return Err(CoffeError::PrioridadCongelada { id, estado: tarea.state.as_str() });
        }
        self.conn.execute(
            "UPDATE tasks SET priority = ?2 WHERE id = ?1",
            params![id, prioridad.as_str()],
        )?;
        Ok(())
    }

    /// La tarea pasa a estar en curso. `first_started_at` se sella una sola
    /// vez: es el arranque del reloj de calendario, no el del último tramo.
    pub fn marcar_en_curso(&self, id: i64, now: DateTime<Utc>) -> Result<(), CoffeError> {
        let n = self.conn.execute(
            "UPDATE tasks
                SET state = 'in_progress',
                    first_started_at = COALESCE(first_started_at, ?2),
                    completed_at = NULL
              WHERE id = ?1",
            params![id, a_texto(now)],
        )?;
        if n == 0 {
            return Err(CoffeError::NoExiste { que: "tarea", id });
        }
        Ok(())
    }

    /// Al refri.
    pub fn aparcar(&self, id: i64) -> Result<(), CoffeError> {
        let n = self.conn.execute(
            "UPDATE tasks SET state = 'paused' WHERE id = ?1 AND state = 'in_progress'",
            params![id],
        )?;
        if n == 0 {
            return Err(CoffeError::NoExiste { que: "tarea en curso", id });
        }
        Ok(())
    }

    pub fn completar(&self, id: i64, now: DateTime<Utc>) -> Result<(), CoffeError> {
        let n = self.conn.execute(
            "UPDATE tasks SET state = 'done', completed_at = ?2 WHERE id = ?1",
            params![id, a_texto(now)],
        )?;
        if n == 0 {
            return Err(CoffeError::NoExiste { que: "tarea", id });
        }
        Ok(())
    }

    /// Vaciar la papelera: las tazas del bote se van. No se borra nada, se
    /// archiva — los tiempos siguen contando para los reportes.
    pub fn vaciar_papelera(&self, now: DateTime<Utc>) -> Result<usize, CoffeError> {
        Ok(self.conn.execute(
            "UPDATE tasks SET state = 'archived', archived_at = ?1 WHERE state = 'done'",
            params![a_texto(now)],
        )?)
    }
}

fn fila_a_tarea(f: &Row<'_>) -> rusqlite::Result<Result<Task, CoffeError>> {
    let priority: String = f.get("priority")?;
    let state: String = f.get("state")?;
    let created_at: String = f.get("created_at")?;
    let first: Option<String> = f.get("first_started_at")?;
    let done: Option<String> = f.get("completed_at")?;

    Ok((|| {
        Ok(Task {
            id: f.get("id")?,
            project_id: f.get("project_id")?,
            title: f.get("title")?,
            notes: f.get("notes")?,
            priority: priority.parse().map_err(|e| CoffeError::DatoCorrupto(format!("{e}")))?,
            state: state.parse().map_err(|e| CoffeError::DatoCorrupto(format!("{e}")))?,
            estimate_pomodoros: f.get("estimate_pomodoros")?,
            due_date: f.get("due_date")?,
            vault_note: f.get("vault_note")?,
            position: f.get("position")?,
            created_at: de_texto(&created_at)?,
            first_started_at: de_texto_opt(first)?,
            completed_at: de_texto_opt(done)?,
        })
    })())
}

impl Db {
    /// Cuántas tazas hay en el bote: terminadas y sin archivar.
    pub fn en_papelera(&self) -> Result<u32, CoffeError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tasks WHERE state = 'done'", [], |f| f.get(0))?)
    }

    /// La última tarea que se mandó al refri. Es lo que retoma `coffe resume`
    /// cuando no se le dice cuál.
    pub fn ultima_aparcada(&self) -> Result<Option<Task>, CoffeError> {
        self.conn
            .query_row(
                "SELECT t.* FROM tasks t
                   JOIN task_sessions s ON s.task_id = t.id
                  WHERE t.state = 'paused' AND s.end_reason IN ('parked','switched')
                  ORDER BY s.ended_at DESC
                  LIMIT 1",
                [],
                fila_a_tarea,
            )
            .optional()?
            .transpose()
    }
}

/// Los cambios que una tarea admite después de creada. Cada `None` deja el
/// campo como estaba; `Some(None)` lo borra. Sin esa distinción no habría
/// forma de quitar una fecha de entrega.
#[derive(Debug, Clone, Default)]
pub struct CambiosTarea {
    pub title: Option<String>,
    pub notes: Option<Option<String>>,
    pub due_date: Option<Option<String>>,
    pub estimate_pomodoros: Option<Option<u32>>,
    pub vault_note: Option<Option<String>>,
}

impl CambiosTarea {
    pub fn vacio(&self) -> bool {
        self.title.is_none()
            && self.notes.is_none()
            && self.due_date.is_none()
            && self.estimate_pomodoros.is_none()
            && self.vault_note.is_none()
    }
}

/// Administración de tareas: editar, mover de proyecto y borrar.
impl Db {
    pub fn editar_tarea(&self, id: i64, cambios: &CambiosTarea) -> Result<(), CoffeError> {
        self.tarea(id)?;
        if cambios.vacio() {
            return Ok(());
        }

        // Se arma con solo los campos pedidos para no pisar con NULL lo que el
        // usuario no menciono.
        let mut trozos: Vec<&str> = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(v) = &cambios.title {
            trozos.push("title = ?");
            vals.push(Box::new(v.clone()));
        }
        if let Some(v) = &cambios.notes {
            trozos.push("notes = ?");
            vals.push(Box::new(v.clone()));
        }
        if let Some(v) = &cambios.due_date {
            trozos.push("due_date = ?");
            vals.push(Box::new(v.clone()));
        }
        if let Some(v) = &cambios.estimate_pomodoros {
            trozos.push("estimate_pomodoros = ?");
            vals.push(Box::new(*v));
        }
        if let Some(v) = &cambios.vault_note {
            trozos.push("vault_note = ?");
            vals.push(Box::new(v.clone()));
        }

        vals.push(Box::new(id));
        let sql = format!("UPDATE tasks SET {} WHERE id = ?", trozos.join(", "));
        let refs: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|b| b.as_ref()).collect();
        self.conn.execute(&sql, refs.as_slice())?;
        Ok(())
    }

    pub fn mover_tarea(&self, id: i64, project_id: i64) -> Result<(), CoffeError> {
        self.tarea(id)?;
        self.proyecto(project_id)?;
        self.conn
            .execute("UPDATE tasks SET project_id = ?2 WHERE id = ?1", params![id, project_id])?;
        Ok(())
    }

    /// Borra la tarea. Sin `force` se niega si tiene pomodoros: el esquema
    /// borra en cascada y con ellos se va el tiempo medido, que es lo único
    /// que esta aplicación no puede reconstruir.
    pub fn borrar_tarea(&self, id: i64, force: bool) -> Result<(), CoffeError> {
        self.tarea(id)?;
        let pomodoros: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM pomodoros WHERE task_id = ?1",
            params![id],
            |f| f.get(0),
        )?;

        if !force && pomodoros > 0 {
            return Err(CoffeError::TareaConHistorial { id, pomodoros });
        }
        self.conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        Ok(())
    }
}
