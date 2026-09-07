//! Lo que mide el tiempo: pomodoros, tramos de tarea, interrupciones, y el
//! estado del reloj para sobrevivir a un reinicio.

use super::{Db, a_texto, de_texto_opt};
use crate::error::CoffeError;
use crate::machine::{SessionEnd, TimerState};
use crate::model::{InterruptionKind, VoidReason};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Todo lo que hace falta saber del tiempo de una tarea. Es la respuesta a las
/// tres preguntas que motivaron el proyecto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumenTarea {
    pub task_id: i64,
    /// Pomodoros que sonaron.
    pub pomodoros_completados: u32,
    /// Pomodoros que se tiraron.
    pub pomodoros_anulados: u32,
    /// Segundos de foco que sí valen.
    pub segundos_efectivos: i64,
    /// Segundos entre el primer arranque y el final, tramos muertos incluidos.
    pub segundos_calendario: Option<i64>,
    /// Segundos de dedicación: la suma de los tramos, descansos incluidos.
    /// Está entre el efectivo y el de calendario.
    pub segundos_en_tramos: i64,
    /// Veces que se aparcó antes de acabar.
    pub veces_aparcada: u32,
    pub interrupciones_internas: u32,
    pub interrupciones_externas: u32,
}

impl Db {
    // ---- pomodoros ----

    /// Abre el pomodoro. El índice parcial del esquema garantiza que no puede
    /// haber dos abiertos a la vez, pase lo que pase con el daemon.
    pub fn abrir_pomodoro(
        &self,
        task_id: i64,
        started_at: DateTime<Utc>,
        planned_secs: i64,
        strict: bool,
    ) -> Result<i64, CoffeError> {
        self.conn.execute(
            "INSERT INTO pomodoros (task_id, started_at, planned_secs, strict)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, a_texto(started_at), planned_secs, strict],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn pomodoro_abierto(&self) -> Result<Option<(i64, i64, DateTime<Utc>)>, CoffeError> {
        let fila = self
            .conn
            .query_row(
                "SELECT id, task_id, started_at FROM pomodoros WHERE outcome IS NULL",
                [],
                |f| Ok((f.get::<_, i64>(0)?, f.get::<_, i64>(1)?, f.get::<_, String>(2)?)),
            )
            .optional()?;

        match fila {
            None => Ok(None),
            Some((id, task_id, ts)) => {
                let started = de_texto_opt(Some(ts))?.expect("texto presente");
                Ok(Some((id, task_id, started)))
            }
        }
    }

    pub fn completar_pomodoro(&self, ended_at: DateTime<Utc>) -> Result<Option<i64>, CoffeError> {
        self.cerrar_pomodoro(ended_at, "completed", None)
    }

    pub fn anular_pomodoro(
        &self,
        ended_at: DateTime<Utc>,
        reason: VoidReason,
    ) -> Result<Option<i64>, CoffeError> {
        self.cerrar_pomodoro(ended_at, "voided", Some(reason))
    }

    fn cerrar_pomodoro(
        &self,
        ended_at: DateTime<Utc>,
        outcome: &str,
        reason: Option<VoidReason>,
    ) -> Result<Option<i64>, CoffeError> {
        let Some((id, _, _)) = self.pomodoro_abierto()? else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE pomodoros SET ended_at = ?2, outcome = ?3, void_reason = ?4 WHERE id = ?1",
            params![id, a_texto(ended_at), outcome, reason.map(|r| r.as_str())],
        )?;
        Ok(Some(id))
    }

    // ---- tramos de tarea ----

    /// Abre un tramo, salvo que ya haya uno abierto para esa tarea. Reabrir
    /// sin cerrar es el bug que haría que el tiempo acumulado creciera solo.
    pub fn abrir_tramo(&self, task_id: i64, now: DateTime<Utc>) -> Result<(), CoffeError> {
        self.conn.execute(
            "INSERT INTO task_sessions (task_id, started_at)
             SELECT ?1, ?2
             WHERE NOT EXISTS (
                 SELECT 1 FROM task_sessions WHERE task_id = ?1 AND ended_at IS NULL
             )",
            params![task_id, a_texto(now)],
        )?;
        Ok(())
    }

    pub fn cerrar_tramo(
        &self,
        task_id: i64,
        now: DateTime<Utc>,
        reason: SessionEnd,
    ) -> Result<(), CoffeError> {
        self.conn.execute(
            "UPDATE task_sessions SET ended_at = ?2, end_reason = ?3
             WHERE task_id = ?1 AND ended_at IS NULL",
            params![task_id, a_texto(now), reason.as_str()],
        )?;
        Ok(())
    }

    /// Cierra cualquier tramo suelto. Lo llama el daemon al arrancar, por si
    /// se fue la luz con una tarea abierta. Quedan marcados como recuperados
    /// para que no se confundan con una pausa de verdad.
    pub fn cerrar_tramos_huerfanos(&self, now: DateTime<Utc>) -> Result<usize, CoffeError> {
        Ok(self.conn.execute(
            "UPDATE task_sessions SET ended_at = ?1, end_reason = 'recovered'
             WHERE ended_at IS NULL",
            params![a_texto(now)],
        )?)
    }

    // ---- interrupciones ----

    pub fn registrar_interrupcion(
        &self,
        task_id: Option<i64>,
        pomodoro_id: Option<i64>,
        kind: InterruptionKind,
        now: DateTime<Utc>,
        note: Option<&str>,
    ) -> Result<i64, CoffeError> {
        self.conn.execute(
            "INSERT INTO interruptions (task_id, pomodoro_id, kind, at, note)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, pomodoro_id, kind.as_str(), a_texto(now), note],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // ---- estado del reloj ----

    pub fn guardar_timer(
        &self,
        state: &TimerState,
        completed_since_long_break: u32,
        now: DateTime<Utc>,
    ) -> Result<(), CoffeError> {
        let json = serde_json::to_string(state)
            .map_err(|e| CoffeError::DatoCorrupto(format!("estado del reloj: {e}")))?;
        self.conn.execute(
            "INSERT INTO timer (id, state_json, completed_since_long_break, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                 state_json = excluded.state_json,
                 completed_since_long_break = excluded.completed_since_long_break,
                 updated_at = excluded.updated_at",
            params![json, completed_since_long_break, a_texto(now)],
        )?;
        Ok(())
    }

    pub fn leer_timer(&self) -> Result<Option<(TimerState, u32)>, CoffeError> {
        let fila = self
            .conn
            .query_row(
                "SELECT state_json, completed_since_long_break FROM timer WHERE id = 1",
                [],
                |f| Ok((f.get::<_, String>(0)?, f.get::<_, u32>(1)?)),
            )
            .optional()?;

        match fila {
            None => Ok(None),
            Some((json, n)) => {
                let state = serde_json::from_str(&json)
                    .map_err(|e| CoffeError::DatoCorrupto(format!("estado del reloj: {e}")))?;
                Ok(Some((state, n)))
            }
        }
    }

    // ---- métricas ----

    pub fn resumen_tarea(&self, task_id: i64) -> Result<ResumenTarea, CoffeError> {
        let tarea = self.tarea(task_id)?;

        let (completados, anulados, efectivos): (u32, u32, i64) = self.conn.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE outcome = 'completed'),
                 COUNT(*) FILTER (WHERE outcome = 'voided'),
                 COALESCE(SUM(planned_secs) FILTER (WHERE outcome = 'completed'), 0)
             FROM pomodoros WHERE task_id = ?1",
            params![task_id],
            |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?)),
        )?;

        // `veces_aparcada` cuenta solo los cierres deliberados: aparcar y
        // cambiar de tarea. El final normal de un ciclo no es una pausa.
        let (aparcada, en_tramos): (u32, i64) = self.conn.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE end_reason IN ('parked','switched')),
                 COALESCE(SUM(
                     CAST(strftime('%s', COALESCE(ended_at, CURRENT_TIMESTAMP)) AS INTEGER)
                   - CAST(strftime('%s', started_at) AS INTEGER)
                 ), 0)
             FROM task_sessions WHERE task_id = ?1",
            params![task_id],
            |f| Ok((f.get(0)?, f.get(1)?)),
        )?;

        let (internas, externas): (u32, u32) = self.conn.query_row(
            "SELECT
                 COUNT(*) FILTER (WHERE kind = 'internal'),
                 COUNT(*) FILTER (WHERE kind = 'external')
             FROM interruptions WHERE task_id = ?1",
            params![task_id],
            |f| Ok((f.get(0)?, f.get(1)?)),
        )?;

        let segundos_calendario = match (tarea.first_started_at, tarea.completed_at) {
            (Some(ini), Some(fin)) => Some((fin - ini).num_seconds()),
            _ => None,
        };

        Ok(ResumenTarea {
            task_id,
            pomodoros_completados: completados,
            pomodoros_anulados: anulados,
            segundos_efectivos: efectivos,
            segundos_calendario,
            segundos_en_tramos: en_tramos,
            veces_aparcada: aparcada,
            interrupciones_internas: internas,
            interrupciones_externas: externas,
        })
    }
}

impl Db {
    /// Pomodoros que sonaron hoy, en día **local**: el día del usuario empieza
    /// cuando se levanta, no a medianoche UTC.
    pub fn pomodoros_hoy(&self, now: DateTime<Utc>) -> Result<u32, CoffeError> {
        let inicio = now
            .with_timezone(&chrono::Local)
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("medianoche siempre existe")
            .and_local_timezone(chrono::Local)
            .earliest()
            // Los días con cambio de horario pueden no tener medianoche. Ese
            // día se cuenta desde las 00:00 UTC y nadie se entera.
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc());

        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM pomodoros WHERE outcome = 'completed' AND ended_at >= ?1",
            params![a_texto(inicio)],
            |f| f.get(0),
        )?)
    }

    /// Pomodoros que ya sonaron para una tarea.
    pub fn pomodoros_de_tarea(&self, task_id: i64) -> Result<u32, CoffeError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM pomodoros WHERE task_id = ?1 AND outcome = 'completed'",
            params![task_id],
            |f| f.get(0),
        )?)
    }
}
