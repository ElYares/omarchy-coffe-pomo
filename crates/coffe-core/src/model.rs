//! Los tipos del dominio. Nada aquí sabe de SQL, de sockets ni de pantallas.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Alta, media y baja. Se congelan en cuanto la tarea recibe su primer
/// pomodoro: ver `RuleViolation::PriorityLocked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
        }
    }

    /// La etiqueta que ve el usuario.
    pub fn etiqueta(self) -> &'static str {
        match self {
            Priority::Low => "baja",
            Priority::Medium => "media",
            Priority::High => "alta",
        }
    }
}

impl FromStr for Priority {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" | "baja" | "b" => Ok(Priority::Low),
            "medium" | "media" | "m" => Ok(Priority::Medium),
            "high" | "alta" | "a" => Ok(Priority::High),
            other => Err(ParseError::new("prioridad", other)),
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// El ciclo de vida de una tarea. `Paused` es el refri: la tarea sigue viva y
/// conserva su tiempo acumulado, pero no hay ningún pomodoro encima.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    InProgress,
    Paused,
    Done,
    Archived,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Pending => "pending",
            TaskState::InProgress => "in_progress",
            TaskState::Paused => "paused",
            TaskState::Done => "done",
            TaskState::Archived => "archived",
        }
    }

    /// Una tarea que ya arrancó alguna vez tiene la prioridad congelada.
    pub fn ya_arranco(self) -> bool {
        !matches!(self, TaskState::Pending)
    }
}

impl FromStr for TaskState {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(TaskState::Pending),
            "in_progress" => Ok(TaskState::InProgress),
            "paused" => Ok(TaskState::Paused),
            "done" => Ok(TaskState::Done),
            "archived" => Ok(TaskState::Archived),
            other => Err(ParseError::new("estado de tarea", other)),
        }
    }
}

/// Corto tras cada pomodoro, largo cada `long_break_every` **completados**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BreakKind {
    Short,
    Long,
}

impl BreakKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BreakKind::Short => "short",
            BreakKind::Long => "long",
        }
    }
}

/// Por qué murió un pomodoro sin sonar. Un pomodoro anulado no cuenta para el
/// descanso largo ni para las métricas de tiempo efectivo, pero sí se guarda:
/// saber cuántos se anulan y por qué es la mitad del valor del método.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoidReason {
    /// Aparcaste la tarea a media pomodoro.
    Paused,
    /// Cambiaste a otra tarea sin terminar esta.
    Switched,
    /// Una interrupción que no se pudo posponer.
    Interrupted,
    /// Marcaste la tarea terminada antes de que sonara.
    FinishedEarly,
    /// Lo tiraste a mano.
    Abandoned,
    /// El daemon se cayó con un foco vivo y al volver ya no era recuperable.
    DaemonLost,
}

impl VoidReason {
    pub fn as_str(self) -> &'static str {
        match self {
            VoidReason::Paused => "paused",
            VoidReason::Switched => "switched",
            VoidReason::Interrupted => "interrupted",
            VoidReason::FinishedEarly => "finished_early",
            VoidReason::Abandoned => "abandoned",
            VoidReason::DaemonLost => "daemon_lost",
        }
    }
}

/// Cirillo las marca distinto en la hoja: la comilla simple es tuya, la doble
/// es de fuera. La distinción importa porque se corrigen de forma distinta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterruptionKind {
    /// `'` — te distrajiste tú.
    Internal,
    /// `"` — te interrumpió algo o alguien.
    External,
}

impl InterruptionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InterruptionKind::Internal => "internal",
            InterruptionKind::External => "external",
        }
    }

    /// La marca de la hoja de seguimiento.
    pub fn marca(self) -> &'static str {
        match self {
            InterruptionKind::Internal => "'",
            InterruptionKind::External => "\"",
        }
    }
}

impl FromStr for InterruptionKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "internal" | "interna" | "i" | "'" => Ok(InterruptionKind::Internal),
            "external" | "externa" | "e" | "\"" => Ok(InterruptionKind::External),
            other => Err(ParseError::new("tipo de interrupción", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    campo: &'static str,
    valor: String,
}

impl ParseError {
    fn new(campo: &'static str, valor: &str) -> Self {
        Self { campo, valor: valor.to_string() }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} desconocida: {:?}", self.campo, self.valor)
    }
}

impl std::error::Error for ParseError {}
