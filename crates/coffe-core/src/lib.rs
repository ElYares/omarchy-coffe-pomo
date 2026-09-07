//! El núcleo de coffe: el dominio del pomodoro y su persistencia.
//!
//! Nada de aquí abre sockets ni pinta ventanas. El daemon, la CLI y la app son
//! clientes de este crate.

pub mod agenda;
pub mod config;
pub mod db;
pub mod error;
pub mod machine;
pub mod model;
pub mod paths;
pub mod service;

pub use config::Config;
pub use db::Db;
pub use error::CoffeError;
pub use machine::{Command, Effect, Machine, RuleViolation, SessionEnd, TimerState};
pub use model::{BreakKind, InterruptionKind, Priority, TaskState, VoidReason};
pub use service::Service;
