//! Cómo se arma el `Snapshot` que ven la barra y la ventana.
//!
//! Separa lo que cambia cada segundo (la cuenta atrás) de lo que solo cambia
//! cuando pasa algo (qué tarea, cuántos pomodoros hoy, cuántas tazas en el
//! bote). Lo segundo cuesta consultas, y la cuenta atrás no tiene por qué
//! pagarlas sesenta veces por minuto.

use anyhow::Result;
use chrono::{DateTime, Utc};
use coffe_core::Service;
use coffe_ipc::{Phase, Snapshot, TaskBrief};

#[derive(Debug, Clone, Default)]
pub struct Base {
    pub task: Option<TaskBrief>,
    pub pomodoros_hoy: u32,
    pub en_papelera: u32,
}

impl Base {
    pub fn leer(svc: &Service, now: DateTime<Utc>) -> Result<Self> {
        let db = svc.db();
        let task = match svc.state().task_id() {
            None => None,
            Some(id) => {
                let t = db.tarea(id)?;
                Some(TaskBrief {
                    id: t.id,
                    title: t.title,
                    project: db.ruta_proyecto(t.project_id)?,
                    priority: t.priority,
                    estimate_pomodoros: t.estimate_pomodoros,
                    done_pomodoros: db.pomodoros_de_tarea(id)?,
                })
            }
        };

        Ok(Self { task, pomodoros_hoy: db.pomodoros_hoy(now)?, en_papelera: db.en_papelera()? })
    }
}

pub fn snapshot(svc: &Service, base: &Base, now: DateTime<Utc>) -> Snapshot {
    let state = svc.state().clone();
    let m = svc.machine();

    // Se redondea hacia arriba: un pomodoro recién arrancado tiene 24
    // minutos y 59,99 segundos por delante, y enseñar 24:59 en el primer
    // instante parece un error aunque no lo sea.
    let restante = state.remaining(now);
    let remaining_secs = (restante.num_milliseconds() + 999) / 1000;

    Snapshot {
        phase: Phase::de_estado(&state),
        remaining_secs,
        progress: state.progress(now),
        strict: m.config().strict,
        completed_since_long_break: m.completed_since_long_break(),
        long_break_every: m.config().long_break_every,
        task: base.task.clone(),
        pomodoros_hoy: base.pomodoros_hoy,
        en_papelera: base.en_papelera,
        now,
        state,
    }
}
