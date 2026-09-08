//! Las preguntas que motivaron el proyecto.
//!
//! No son estadísticas por tener estadísticas: cada una responde algo que sin
//! medir solo se puede opinar. Cuánto costó de verdad una tarea, cuánto se te
//! va en cada proyecto, y si tus estimaciones sirven para algo.

use super::tasks::FiltroTareas;
use super::{Db, a_texto};
use crate::error::CoffeError;
use crate::model::TaskState;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// Lo que pasó en un tramo de tiempo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumenPeriodo {
    pub pomodoros_completados: u32,
    /// Los que se tiraron. La proporción dice más del día que el total.
    pub pomodoros_anulados: u32,
    pub segundos_efectivos: i64,
    pub tareas_terminadas: u32,
    pub interrupciones_internas: u32,
    pub interrupciones_externas: u32,
    /// Días distintos con al menos un pomodoro. Sirve para leer el total: 40
    /// pomodoros en cuatro días no es lo mismo que en veinte.
    pub dias_con_trabajo: u32,
    pub segundos_con_claude: i64,
}

impl ResumenPeriodo {
    /// Qué proporción de los pomodoros empezados acabó en la basura.
    pub fn tasa_de_anulacion(&self) -> f64 {
        let total = self.pomodoros_completados + self.pomodoros_anulados;
        if total == 0 {
            return 0.0;
        }
        self.pomodoros_anulados as f64 / total as f64
    }
}

/// Cuánto se lleva cada proyecto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargaProyecto {
    pub project_id: i64,
    pub ruta: String,
    pub pomodoros: u32,
    pub anulados: u32,
    pub segundos_efectivos: i64,
}

/// Si tus estimaciones sirven de algo.
///
/// Solo mira tareas **terminadas** y **con estimación**: una a medias todavía
/// puede crecer, y sin estimación no hay nada que comparar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Precision {
    pub tareas: u32,
    pub estimados: u32,
    pub reales: u32,
    /// Costaron más de lo dicho.
    pub subestimadas: u32,
    pub clavadas: u32,
    /// Costaron menos.
    pub sobreestimadas: u32,
    /// Tareas terminadas que NO tenían estimación. No entran en ninguna de las
    /// cuentas de arriba, y por eso hay que enseñarlas: un factor calculado
    /// sobre tres tareas mientras otras veinte se terminaron sin estimar se lee
    /// como si describiera tu forma de estimar, y describe una esquina.
    pub sin_estimar: u32,
}

impl Precision {
    /// Por cuánto multiplicar una estimación tuya para acercarte a la realidad.
    /// `None` cuando no hay con qué compararse.
    pub fn factor(&self) -> Option<f64> {
        if self.tareas == 0 || self.estimados == 0 {
            return None;
        }
        Some(self.reales as f64 / self.estimados as f64)
    }
}

impl Db {
    pub fn resumen_periodo(
        &self,
        desde: DateTime<Utc>,
        hasta: DateTime<Utc>,
    ) -> Result<ResumenPeriodo, CoffeError> {
        let (d, h) = (a_texto(desde), a_texto(hasta));

        let (completados, anulados, efectivos, dias): (u32, u32, i64, u32) = self.conn.query_row(
            "SELECT
                     COUNT(*) FILTER (WHERE outcome = 'completed'),
                     COUNT(*) FILTER (WHERE outcome = 'voided'),
                     COALESCE(SUM(planned_secs) FILTER (WHERE outcome = 'completed'), 0),
                     COUNT(DISTINCT date(ended_at)) FILTER (WHERE outcome = 'completed')
                 FROM pomodoros
                 WHERE ended_at >= ?1 AND ended_at < ?2",
            params![d, h],
            |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?, f.get(3)?)),
        )?;

        let terminadas: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE completed_at >= ?1 AND completed_at < ?2",
            params![d, h],
            |f| f.get(0),
        )?;

        let (internas, externas): (u32, u32) = self.conn.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE kind = 'internal'),
                 COUNT(*) FILTER (WHERE kind = 'external')
             FROM interruptions WHERE at >= ?1 AND at < ?2",
            params![d, h],
            |f| Ok((f.get(0)?, f.get(1)?)),
        )?;

        let con_claude: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(
                 CAST(strftime('%s', ended_at) AS INTEGER)
               - CAST(strftime('%s', started_at) AS INTEGER)
             ), 0)
             FROM claude_spans
             WHERE ended_at IS NOT NULL AND ended_at >= ?1 AND ended_at < ?2",
            params![d, h],
            |f| f.get(0),
        )?;

        Ok(ResumenPeriodo {
            pomodoros_completados: completados,
            pomodoros_anulados: anulados,
            segundos_efectivos: efectivos,
            tareas_terminadas: terminadas,
            interrupciones_internas: internas,
            interrupciones_externas: externas,
            dias_con_trabajo: dias,
            segundos_con_claude: con_claude,
        })
    }

    /// El tiempo repartido por proyecto, de más a menos.
    ///
    /// Cada proyecto suma **lo suyo y lo de sus hijos**: preguntar cuánto se
    /// lleva `strapp` y que no cuente lo de `strapp / tl-mas` sería una
    /// jerarquía de adorno.
    pub fn carga_por_proyecto(
        &self,
        desde: DateTime<Utc>,
        hasta: DateTime<Utc>,
    ) -> Result<Vec<CargaProyecto>, CoffeError> {
        let (d, h) = (a_texto(desde), a_texto(hasta));
        let mut cargas = Vec::new();

        for p in self.proyectos(true)? {
            let mut ids = self.descendientes(p.id)?;
            ids.push(p.id);
            let marcas = vec!["?"; ids.len()].join(",");

            let sql = format!(
                "SELECT
                     COUNT(*) FILTER (WHERE po.outcome = 'completed'),
                     COUNT(*) FILTER (WHERE po.outcome = 'voided'),
                     COALESCE(SUM(po.planned_secs) FILTER (WHERE po.outcome = 'completed'), 0)
                 FROM pomodoros po
                 JOIN tasks t ON t.id = po.task_id
                 WHERE t.project_id IN ({marcas})
                   AND po.ended_at >= ? AND po.ended_at < ?"
            );

            let mut vals: Vec<&dyn rusqlite::ToSql> =
                ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
            vals.push(&d);
            vals.push(&h);

            let (pomodoros, anulados, efectivos): (u32, u32, i64) =
                self.conn
                    .query_row(&sql, vals.as_slice(), |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?)))?;

            if pomodoros > 0 || anulados > 0 {
                cargas.push(CargaProyecto {
                    project_id: p.id,
                    ruta: self.ruta_proyecto(p.id)?,
                    pomodoros,
                    anulados,
                    segundos_efectivos: efectivos,
                });
            }
        }

        cargas.sort_by_key(|c| std::cmp::Reverse(c.segundos_efectivos));
        Ok(cargas)
    }

    pub fn precision_estimacion(&self) -> Result<Precision, CoffeError> {
        let sin_estimar: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM tasks
             WHERE completed_at IS NOT NULL AND estimate_pomodoros IS NULL",
            [],
            |f| f.get(0),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT
                 t.estimate_pomodoros,
                 (SELECT COUNT(*) FROM pomodoros p
                   WHERE p.task_id = t.id AND p.outcome = 'completed')
             FROM tasks t
             WHERE t.completed_at IS NOT NULL AND t.estimate_pomodoros IS NOT NULL",
        )?;

        let filas: Vec<(u32, u32)> =
            stmt.query_map([], |f| Ok((f.get(0)?, f.get(1)?)))?.collect::<Result<_, _>>()?;

        let mut r = Precision { sin_estimar, ..Default::default() };
        for (estimado, real) in filas {
            r.tareas += 1;
            r.estimados += estimado;
            r.reales += real;
            match real.cmp(&estimado) {
                std::cmp::Ordering::Greater => r.subestimadas += 1,
                std::cmp::Ordering::Equal => r.clavadas += 1,
                std::cmp::Ordering::Less => r.sobreestimadas += 1,
            }
        }
        Ok(r)
    }
}

// ------------------------------------------------------------------- CSV

/// Qué se saca.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exportacion {
    /// Un renglón por pomodoro del periodo.
    Pomodoros { dias: u32 },
    /// Un renglón por tarea, con sus tres medidas del tiempo.
    Tareas,
}

/// Un campo CSV. Los títulos llevan comas y comillas más a menudo de lo que
/// parece, y una sola sin escapar corre todas las columnas de esa fila.
fn campo(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Un pomodoro, aplanado para exportarlo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilaPomodoro {
    pub started_at: String,
    pub ended_at: Option<String>,
    pub outcome: Option<String>,
    pub void_reason: Option<String>,
    pub planned_secs: i64,
    pub strict: bool,
    pub task_id: i64,
    pub titulo: String,
    pub proyecto: String,
}

impl Db {
    /// Los pomodoros de un periodo, con su tarea y su proyecto ya resueltos.
    /// Es lo que se exporta a CSV.
    pub fn pomodoros_del_periodo(
        &self,
        desde: DateTime<Utc>,
        hasta: DateTime<Utc>,
    ) -> Result<Vec<FilaPomodoro>, CoffeError> {
        let mut stmt = self.conn.prepare(
            "SELECT p.started_at, p.ended_at, p.outcome, p.void_reason, p.planned_secs,
                    p.strict, t.id, t.title, t.project_id
             FROM pomodoros p JOIN tasks t ON t.id = p.task_id
             WHERE p.ended_at >= ?1 AND p.ended_at < ?2
             ORDER BY p.started_at",
        )?;

        // La fila cruda, tal como sale de SQLite. El nombre existe para que
        // el tipo no ocupe diez líneas en medio de la consulta.
        type Cruda =
            (String, Option<String>, Option<String>, Option<String>, i64, i64, i64, String, i64);

        let crudas: Vec<Cruda> = stmt
            .query_map(params![a_texto(desde), a_texto(hasta)], |f| {
                Ok((
                    f.get(0)?,
                    f.get(1)?,
                    f.get(2)?,
                    f.get(3)?,
                    f.get(4)?,
                    f.get(5)?,
                    f.get(6)?,
                    f.get(7)?,
                    f.get(8)?,
                ))
            })?
            .collect::<Result<_, _>>()?;

        crudas
            .into_iter()
            .map(|(ini, fin, res, motivo, secs, estricto, tid, titulo, pid)| {
                Ok(FilaPomodoro {
                    started_at: ini,
                    ended_at: fin,
                    outcome: res,
                    void_reason: motivo,
                    planned_secs: secs,
                    strict: estricto != 0,
                    task_id: tid,
                    titulo,
                    proyecto: self.ruta_proyecto(pid)?,
                })
            })
            .collect()
    }
}

impl Db {
    /// El CSV entero, cabecera incluida, y cuántas filas lleva.
    ///
    /// Lo arma el núcleo y no cada interfaz. La terminal lo imprime y la
    /// ventana lo escribe en un archivo, pero **el texto es el mismo**: dos
    /// escapadores escritos por separado acaban produciendo dos archivos
    /// distintos para los mismos datos, y el que se lleva el susto es quien
    /// abra el que escapaba peor.
    ///
    /// Devuelve las filas aparte porque un archivo de cero filas escrito sin
    /// una queja es la clase de éxito que se descubre tarde.
    pub fn csv(&self, que: Exportacion) -> Result<(String, usize), CoffeError> {
        let (cabecera, filas) = match que {
            Exportacion::Pomodoros { dias } => {
                let hasta = Utc::now();
                let desde = hasta - chrono::Duration::days(dias.clamp(1, 3650) as i64);
                let filas = self
                    .pomodoros_del_periodo(desde, hasta)?
                    .into_iter()
                    .map(|f| {
                        vec![
                            f.started_at,
                            f.ended_at.unwrap_or_default(),
                            f.outcome.unwrap_or_default(),
                            f.void_reason.unwrap_or_default(),
                            f.planned_secs.to_string(),
                            if f.strict { "si".into() } else { "no".into() },
                            f.titulo,
                            f.proyecto,
                        ]
                    })
                    .collect::<Vec<_>>();
                ("inicio,fin,resultado,motivo,segundos,estricto,tarea,proyecto", filas)
            }

            Exportacion::Tareas => {
                // Todos los estados, archivadas incluidas: una exportación que
                // se deja fuera lo archivado pierde justo la historia vieja,
                // que es para lo que se exporta.
                let todos = vec![
                    TaskState::Pending,
                    TaskState::InProgress,
                    TaskState::Paused,
                    TaskState::Done,
                    TaskState::Archived,
                ];
                let tareas =
                    self.tareas(&FiltroTareas { states: Some(todos), ..Default::default() })?;

                let mut filas = Vec::with_capacity(tareas.len());
                for t in tareas {
                    let r = self.resumen_tarea(t.id)?;
                    filas.push(vec![
                        t.id.to_string(),
                        t.title,
                        self.ruta_proyecto(t.project_id).unwrap_or_default(),
                        t.state.as_str().to_string(),
                        t.priority.as_str().to_string(),
                        t.due_date.unwrap_or_default(),
                        t.estimate_pomodoros.map(|e| e.to_string()).unwrap_or_default(),
                        r.pomodoros_completados.to_string(),
                        r.pomodoros_anulados.to_string(),
                        r.segundos_efectivos.to_string(),
                        r.segundos_en_tramos.to_string(),
                        r.segundos_calendario.map(|s| s.to_string()).unwrap_or_default(),
                        r.veces_aparcada.to_string(),
                        r.interrupciones_internas.to_string(),
                        r.interrupciones_externas.to_string(),
                        r.segundos_con_claude.to_string(),
                        t.vault_note.unwrap_or_default(),
                    ]);
                }
                (
                    "id,titulo,proyecto,estado,prioridad,entrega,estimados,completados,anulados,\
                     seg_efectivos,seg_dedicacion,seg_calendario,pausas,interrup_internas,\
                     interrup_externas,seg_claude,nota",
                    filas,
                )
            }
        };

        let mut texto = String::from(cabecera);
        texto.push('\n');
        for fila in &filas {
            texto.push_str(&fila.iter().map(|c| campo(c)).collect::<Vec<_>>().join(","));
            texto.push('\n');
        }
        Ok((texto, filas.len()))
    }
}
