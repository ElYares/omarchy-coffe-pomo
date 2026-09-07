//! El contrato entre el daemon y sus clientes (la CLI y la ventana).
//!
//! Vive en su propio crate para que el protocolo tenga un solo dueño: si la
//! ventana y el daemon lo definieran cada uno por su lado, se separarían.
//!
//! El transporte es un socket unix con **JSON por líneas**: una petición por
//! línea, una respuesta por línea. Se eligió así porque se puede depurar con
//! `socat` y porque el cliente no necesita runtime asíncrono — y el cliente
//! que más corre es el módulo de waybar, donde cada milisegundo de arranque
//! se ve.

pub mod client;

use chrono::{DateTime, Utc};
use coffe_core::machine::TimerState;
use coffe_core::model::{BreakKind, InterruptionKind, Priority, VoidReason};
use serde::{Deserialize, Serialize};

/// Lo que un cliente le pide al daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "req", rename_all = "snake_case")]
pub enum Request {
    /// Cómo está todo ahora mismo.
    Status,
    Start {
        task_id: i64,
    },
    Pause,
    Void {
        reason: VoidReason,
    },
    Switch {
        task_id: i64,
    },
    Done,
    Interrupt {
        kind: InterruptionKind,
        note: Option<String>,
    },
    SkipBreak,
    /// Deja la conexión abierta y recibe un `Snapshot` por línea: en cada
    /// transición, y cada segundo mientras haya reloj corriendo. Es lo que usa
    /// la barra para la cuenta atrás sin arrancar un proceso por segundo.
    Subscribe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "res", rename_all = "snake_case")]
pub enum Response {
    Ok {
        snapshot: Box<Snapshot>,
    },
    /// La petición era válida pero rompía una regla del método, o falló algo.
    Error {
        message: String,
    },
}

/// La fase del reloj, ya masticada. El estado completo va aparte; esto es lo
/// que la barra necesita para elegir clase de CSS e icono.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Focus,
    /// La tarea ya está hecha y el pomodoro corre su repaso.
    Overlearning,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Focus => "focus",
            Phase::Overlearning => "overlearning",
            Phase::ShortBreak => "short_break",
            Phase::LongBreak => "long_break",
        }
    }

    pub fn de_estado(state: &TimerState) -> Self {
        match state {
            TimerState::Idle => Phase::Idle,
            TimerState::Focus { overlearning: true, .. } => Phase::Overlearning,
            TimerState::Focus { .. } => Phase::Focus,
            TimerState::Break { kind: BreakKind::Short, .. } => Phase::ShortBreak,
            TimerState::Break { kind: BreakKind::Long, .. } => Phase::LongBreak,
        }
    }
}

/// Todo lo que hace falta para pintar la barra o la ventana, en un solo viaje.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: TimerState,
    pub phase: Phase,
    pub remaining_secs: i64,
    /// De 0.0 a 1.0. Es el nivel de la taza: en foco baja, en descanso sube.
    pub progress: f64,
    pub strict: bool,
    /// Pomodoros completados desde el último descanso largo.
    pub completed_since_long_break: u32,
    pub long_break_every: u32,
    pub task: Option<TaskBrief>,
    /// Pomodoros que sonaron hoy, en día local.
    pub pomodoros_hoy: u32,
    /// Tazas en el bote: tareas terminadas sin archivar.
    pub en_papelera: u32,
    pub now: DateTime<Utc>,
}

impl Snapshot {
    /// `12:34`, o `--:--` cuando no hay reloj.
    pub fn reloj(&self) -> String {
        if self.phase == Phase::Idle {
            return "--:--".to_string();
        }
        format!("{:02}:{:02}", self.remaining_secs / 60, self.remaining_secs % 60)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBrief {
    pub id: i64,
    pub title: String,
    /// La ruta legible del proyecto: `clientes / nutricore`.
    pub project: String,
    pub priority: Priority,
    pub estimate_pomodoros: Option<u32>,
    /// Pomodoros que ya sonaron para esta tarea.
    pub done_pomodoros: u32,
}
