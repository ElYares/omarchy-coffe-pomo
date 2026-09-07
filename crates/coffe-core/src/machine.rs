//! La máquina de estados del pomodoro.
//!
//! Es una función pura: recibe un comando y el instante en que ocurre, y
//! devuelve los efectos que hay que persistir y notificar. No mira el reloj del
//! sistema ni toca la base — por eso se puede probar cada regla sin esperar
//! veinticinco minutos.
//!
//! Las reglas que impone, todas de Cirillo:
//!
//! - **El pomodoro es indivisible.** No hay pausa. Interrumpirlo lo anula.
//! - **Si un pomodoro empieza, tiene que sonar.** Terminar la tarea antes no
//!   corta el reloj: el tiempo que sobra es para repasar (`overlearning`).
//! - **El descanso es obligatorio.** En modo estricto no se puede abrir un foco
//!   mientras corre un descanso, ni saltárselo.
//! - **Descanso largo cada N completados.** Los anulados no cuentan.

use crate::config::Pomodoro;
use crate::model::{BreakKind, InterruptionKind, VoidReason};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Cuánto puede retrasarse un tick antes de que dejemos de creernos que el
/// pomodoro sonó. Si la máquina estuvo suspendida media hora, la campana no la
/// oyó nadie: ese pomodoro se anula, no se regala.
pub const GRACIA_CAMPANA: Duration = Duration::seconds(120);

/// Dónde está el reloj. No existe un estado "pausado": esa es la diferencia
/// entre el pomodoro y la tarea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TimerState {
    Idle,
    Focus {
        task_id: i64,
        started_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        /// La tarea se marcó terminada pero el pomodoro sigue hasta sonar.
        overlearning: bool,
    },
    Break {
        kind: BreakKind,
        started_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        /// La tarea que se venía trabajando, si sigue viva. Queda en `None`
        /// cuando se terminó o se aparcó: el descanso sigue, pero ya no cuelga
        /// de ninguna tarea.
        after_task: Option<i64>,
    },
}

impl TimerState {
    pub fn is_idle(&self) -> bool {
        matches!(self, TimerState::Idle)
    }

    /// Cuánto falta. Nunca negativo: un reloj vencido devuelve cero.
    pub fn remaining(&self, now: DateTime<Utc>) -> Duration {
        let ends_at = match self {
            TimerState::Idle => return Duration::zero(),
            TimerState::Focus { ends_at, .. } | TimerState::Break { ends_at, .. } => *ends_at,
        };
        (ends_at - now).max(Duration::zero())
    }

    /// El instante en que este estado vence, si vence.
    pub fn ends_at(&self) -> Option<DateTime<Utc>> {
        match self {
            TimerState::Idle => None,
            TimerState::Focus { ends_at, .. } | TimerState::Break { ends_at, .. } => Some(*ends_at),
        }
    }

    /// Fracción consumida, de 0.0 a 1.0. Es el nivel de la taza: en foco baja,
    /// en descanso se llena.
    pub fn progress(&self, now: DateTime<Utc>) -> f64 {
        let (started_at, ends_at) = match self {
            TimerState::Idle => return 0.0,
            TimerState::Focus { started_at, ends_at, .. }
            | TimerState::Break { started_at, ends_at, .. } => (*started_at, *ends_at),
        };
        let total = (ends_at - started_at).num_milliseconds();
        if total <= 0 {
            return 1.0;
        }
        let ido = (now - started_at).num_milliseconds();
        (ido as f64 / total as f64).clamp(0.0, 1.0)
    }

    /// La tarea sobre la que se está trabajando, si hay alguna.
    pub fn task_id(&self) -> Option<i64> {
        match self {
            TimerState::Idle => None,
            TimerState::Focus { task_id, .. } => Some(*task_id),
            TimerState::Break { after_task, .. } => *after_task,
        }
    }
}

/// Lo que el usuario (o un hook) le pide al reloj.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Arranca un pomodoro sobre una tarea.
    Start { task_id: i64 },
    /// Aparca la tarea: al refri. Anula el pomodoro en curso.
    Pause,
    /// Tira el pomodoro en curso sin tocar la tarea.
    Void { reason: VoidReason },
    /// Cambia de tarea en caliente: anula, aparca la vieja y abre la nueva.
    Switch { task_id: i64 },
    /// La tarea quedó terminada.
    Done,
    /// Apunta una interrupción sin cortar el pomodoro.
    Interrupt { kind: InterruptionKind },
    /// Corta el descanso. Solo fuera del modo estricto.
    SkipBreak,
    /// Que el reloj mire la hora. Es lo que hace sonar el pomodoro.
    Tick,
}

/// Lo que hay que escribir y avisar. La máquina no persiste: lo describe.
///
/// Cada variante corresponde a **una** escritura. El estado de la tarea y su
/// tramo de tiempo son cosas distintas y por eso van en efectos distintos: al
/// terminar una tarea a media pomodoro, el estado cambia ya pero el tramo
/// sigue abierto hasta que el reloj suene.
///
/// Los efectos que cierran algo llevan su propio `at`, que **no** es siempre
/// el `now` del comando: un tick que llega tarde tiene que apuntar el final
/// del pomodoro en el minuto 25, no en el minuto que se descubrió.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    FocusStarted {
        task_id: i64,
        ends_at: DateTime<Utc>,
    },
    /// Sonó. Este sí cuenta.
    FocusCompleted {
        task_id: i64,
        at: DateTime<Utc>,
    },
    FocusVoided {
        task_id: i64,
        reason: VoidReason,
        elapsed_secs: i64,
        at: DateTime<Utc>,
    },
    BreakStarted {
        kind: BreakKind,
        ends_at: DateTime<Utc>,
    },
    BreakEnded {
        kind: BreakKind,
        at: DateTime<Utc>,
    },

    // -- estado de la tarea --
    /// Pasa a estar en curso.
    TaskStarted {
        task_id: i64,
    },
    /// Al refri: sigue viva, con su tiempo acumulado, pero nadie la trabaja.
    TaskParked {
        task_id: i64,
    },
    TaskCompleted {
        task_id: i64,
    },

    // -- tramos de tiempo --
    SessionOpened {
        task_id: i64,
    },
    SessionClosed {
        task_id: i64,
        reason: SessionEnd,
        at: DateTime<Utc>,
    },

    InterruptionLogged {
        task_id: Option<i64>,
        kind: InterruptionKind,
    },
}

/// Por qué se cerró un tramo. Distingue el abandono deliberado del simple
/// final de un ciclo, que es lo que hace que "cuántas veces se aparcó" sea un
/// número real y no el número de pomodoros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEnd {
    /// Se aparcó a propósito.
    Parked,
    /// Se cambió a otra tarea.
    Switched,
    /// Quedó terminada.
    Done,
    /// Se acabó el ciclo y el reloj volvió a cero sin decisión de por medio.
    Idle,
    /// Se encontró abierto tras una caída o una suspensión.
    Recovered,
}

impl SessionEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionEnd::Parked => "parked",
            SessionEnd::Switched => "switched",
            SessionEnd::Done => "done",
            SessionEnd::Idle => "idle",
            SessionEnd::Recovered => "recovered",
        }
    }

    /// Si este final cuenta como una pausa de la tarea.
    pub fn es_pausa(self) -> bool {
        matches!(self, SessionEnd::Parked | SessionEnd::Switched)
    }
}

/// Reglas que el comando no puede saltarse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleViolation {
    #[error("ya hay un pomodoro corriendo sobre la tarea {task_id}")]
    AlreadyFocused { task_id: i64 },
    #[error("estás en descanso: el pomodoro no arranca hasta que termine")]
    BreakInProgress { ends_at: DateTime<Utc> },
    #[error("el descanso no se salta en modo estricto")]
    BreakMandatory,
    #[error("no hay nada corriendo")]
    NothingRunning,
    #[error("no hay ninguna tarea en curso")]
    NoTaskInProgress,
}

/// El reloj con su memoria: cuántos pomodoros lleva desde el último descanso
/// largo. Se serializa entera para sobrevivir a un reinicio del daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    cfg: Pomodoro,
    state: TimerState,
    completed_since_long_break: u32,
}

impl Machine {
    pub fn new(cfg: Pomodoro) -> Self {
        Self { cfg, state: TimerState::Idle, completed_since_long_break: 0 }
    }

    /// Reconstruye la máquina tal como estaba. Lo usa el daemon al arrancar.
    pub fn restore(cfg: Pomodoro, state: TimerState, completed_since_long_break: u32) -> Self {
        Self { cfg, state, completed_since_long_break }
    }

    pub fn state(&self) -> &TimerState {
        &self.state
    }

    pub fn config(&self) -> &Pomodoro {
        &self.cfg
    }

    pub fn completed_since_long_break(&self) -> u32 {
        self.completed_since_long_break
    }

    /// Cuántos pomodoros faltan para el descanso largo.
    pub fn until_long_break(&self) -> u32 {
        self.cfg.long_break_every.saturating_sub(self.completed_since_long_break)
    }

    pub fn apply(
        &mut self,
        cmd: Command,
        now: DateTime<Utc>,
    ) -> Result<Vec<Effect>, RuleViolation> {
        // Un comando siempre llega después del reloj: primero se resuelve lo
        // que ya venció, y solo entonces se atiende lo que se pide. Si no, un
        // `start` mandado cinco minutos tarde arrancaría encima de un foco que
        // en realidad ya había sonado.
        let mut fx = self.avanzar_reloj(now);
        match cmd {
            Command::Tick => {}
            otro => fx.extend(self.ejecutar(otro, now)?),
        }
        Ok(fx)
    }

    /// Vence lo que tenga que vencer. Es la única parte que mira `now` contra
    /// los finales, y se aplica en bucle porque un descanso puede vencer en el
    /// mismo tick en que venció el foco que lo abrió.
    fn avanzar_reloj(&mut self, now: DateTime<Utc>) -> Vec<Effect> {
        let mut fx = Vec::new();
        loop {
            match self.state.clone() {
                TimerState::Focus { task_id, started_at, ends_at, overlearning }
                    if now >= ends_at =>
                {
                    // La campana sonó hace demasiado: la máquina estaba
                    // suspendida o el daemon caído, y nadie la oyó. Regalar el
                    // pomodoro sería la primera mentira de la serie.
                    if now - ends_at > GRACIA_CAMPANA {
                        fx.push(Effect::FocusVoided {
                            task_id,
                            reason: VoidReason::DaemonLost,
                            elapsed_secs: (ends_at - started_at).num_seconds().max(0),
                            at: ends_at,
                        });
                        fx.push(Effect::SessionClosed {
                            task_id,
                            reason: SessionEnd::Recovered,
                            at: ends_at,
                        });
                        self.state = TimerState::Idle;
                        break;
                    }

                    fx.push(Effect::FocusCompleted { task_id, at: ends_at });
                    self.completed_since_long_break += 1;

                    // Si la tarea ya se había dado por terminada, el tiempo que
                    // sobró era de repaso: el tramo se cierra aquí, con el
                    // pomodoro, y el descanso ya no cuelga de nada.
                    let after_task = if overlearning {
                        fx.push(Effect::SessionClosed {
                            task_id,
                            reason: SessionEnd::Done,
                            at: ends_at,
                        });
                        None
                    } else {
                        Some(task_id)
                    };

                    let kind = if self.completed_since_long_break >= self.cfg.long_break_every {
                        self.completed_since_long_break = 0;
                        BreakKind::Long
                    } else {
                        BreakKind::Short
                    };
                    let dur = self.duracion_descanso(kind);
                    self.state = TimerState::Break {
                        kind,
                        started_at: ends_at,
                        ends_at: ends_at + dur,
                        after_task,
                    };
                    fx.push(Effect::BreakStarted { kind, ends_at: ends_at + dur });
                }
                TimerState::Break { kind, ends_at, after_task, .. } if now >= ends_at => {
                    self.state = TimerState::Idle;
                    fx.push(Effect::BreakEnded { kind, at: ends_at });
                    // El ciclo terminó: el tramo se cierra aquí, no mañana
                    // cuando alguien se acuerde de aparcar la tarea.
                    if let Some(task_id) = after_task {
                        fx.push(Effect::SessionClosed {
                            task_id,
                            reason: SessionEnd::Idle,
                            at: ends_at,
                        });
                    }
                }
                _ => break,
            }
        }
        fx
    }

    fn ejecutar(&mut self, cmd: Command, now: DateTime<Utc>) -> Result<Vec<Effect>, RuleViolation> {
        match cmd {
            Command::Tick => Ok(Vec::new()),

            Command::Start { task_id } => match self.state.clone() {
                TimerState::Focus { task_id: actual, .. } => {
                    Err(RuleViolation::AlreadyFocused { task_id: actual })
                }
                // El descanso manda. Fuera del estricto se puede cortar, pero
                // queda apuntado como corte, no como descanso cumplido.
                TimerState::Break { ends_at, .. } if self.cfg.strict => {
                    Err(RuleViolation::BreakInProgress { ends_at })
                }
                TimerState::Break { kind, after_task, .. } => {
                    let mut fx = vec![Effect::BreakEnded { kind, at: now }];
                    if let Some(previa) = after_task
                        && previa != task_id
                    {
                        fx.push(Effect::SessionClosed {
                            task_id: previa,
                            reason: SessionEnd::Switched,
                            at: now,
                        });
                        fx.push(Effect::TaskParked { task_id: previa });
                    }
                    self.state = TimerState::Idle;
                    fx.extend(self.abrir_foco(task_id, now));
                    Ok(fx)
                }
                TimerState::Idle => Ok(self.abrir_foco(task_id, now)),
            },

            Command::Pause => match self.state.clone() {
                TimerState::Focus { task_id, started_at, .. } => {
                    let mut fx = vec![self.anular(task_id, started_at, VoidReason::Paused, now)];
                    self.state = TimerState::Idle;
                    fx.push(Effect::SessionClosed { task_id, reason: SessionEnd::Parked, at: now });
                    fx.push(Effect::TaskParked { task_id });
                    Ok(fx)
                }
                // En descanso no hay pomodoro que anular: solo se cierra el
                // tramo de la tarea y el descanso sigue su curso.
                TimerState::Break { after_task: Some(task_id), kind, started_at, ends_at } => {
                    self.state = TimerState::Break { kind, started_at, ends_at, after_task: None };
                    Ok(vec![
                        Effect::SessionClosed { task_id, reason: SessionEnd::Parked, at: now },
                        Effect::TaskParked { task_id },
                    ])
                }
                _ => Err(RuleViolation::NothingRunning),
            },

            Command::Void { reason } => match self.state.clone() {
                TimerState::Focus { task_id, started_at, .. } => {
                    // La tarea sigue en curso: tirar el pomodoro no la aparca.
                    // El tramo sí se cierra, porque el reloj se paró.
                    let fx = vec![
                        self.anular(task_id, started_at, reason, now),
                        Effect::SessionClosed { task_id, reason: SessionEnd::Idle, at: now },
                    ];
                    self.state = TimerState::Idle;
                    Ok(fx)
                }
                _ => Err(RuleViolation::NothingRunning),
            },

            Command::Switch { task_id: nueva } => match self.state.clone() {
                TimerState::Focus { task_id: vieja, started_at, .. } => {
                    let mut fx = vec![self.anular(vieja, started_at, VoidReason::Switched, now)];
                    fx.push(Effect::SessionClosed {
                        task_id: vieja,
                        reason: SessionEnd::Switched,
                        at: now,
                    });
                    fx.push(Effect::TaskParked { task_id: vieja });
                    self.state = TimerState::Idle;
                    fx.extend(self.abrir_foco(nueva, now));
                    Ok(fx)
                }
                TimerState::Break { ends_at, .. } if self.cfg.strict => {
                    Err(RuleViolation::BreakInProgress { ends_at })
                }
                TimerState::Break { kind, after_task, .. } => {
                    let mut fx = vec![Effect::BreakEnded { kind, at: now }];
                    if let Some(vieja) = after_task {
                        fx.push(Effect::SessionClosed {
                            task_id: vieja,
                            reason: SessionEnd::Switched,
                            at: now,
                        });
                        fx.push(Effect::TaskParked { task_id: vieja });
                    }
                    self.state = TimerState::Idle;
                    fx.extend(self.abrir_foco(nueva, now));
                    Ok(fx)
                }
                TimerState::Idle => Ok(self.abrir_foco(nueva, now)),
            },

            Command::Done => match self.state.clone() {
                // Ya estaba marcada; el reloj sigue corriendo su repaso.
                TimerState::Focus { overlearning: true, .. } => {
                    Err(RuleViolation::NoTaskInProgress)
                }
                // "Si un pomodoro empieza, tiene que sonar": la tarea se marca
                // terminada, pero el reloj sigue. El tiempo que sobra es para
                // repasar lo hecho, no para saltar a lo siguiente.
                TimerState::Focus { task_id, started_at, ends_at, .. } if self.cfg.strict => {
                    self.state =
                        TimerState::Focus { task_id, started_at, ends_at, overlearning: true };
                    Ok(vec![Effect::TaskCompleted { task_id }])
                }
                TimerState::Focus { task_id, started_at, .. } => {
                    let mut fx =
                        vec![self.anular(task_id, started_at, VoidReason::FinishedEarly, now)];
                    self.state = TimerState::Idle;
                    fx.push(Effect::SessionClosed { task_id, reason: SessionEnd::Done, at: now });
                    fx.push(Effect::TaskCompleted { task_id });
                    Ok(fx)
                }
                TimerState::Break { after_task: Some(task_id), kind, started_at, ends_at } => {
                    self.state = TimerState::Break { kind, started_at, ends_at, after_task: None };
                    Ok(vec![
                        Effect::SessionClosed { task_id, reason: SessionEnd::Done, at: now },
                        Effect::TaskCompleted { task_id },
                    ])
                }
                _ => Err(RuleViolation::NoTaskInProgress),
            },

            Command::Interrupt { kind } => match &self.state {
                TimerState::Idle => Err(RuleViolation::NothingRunning),
                // Apuntarla es todo lo que hace: informar, negociar y volver
                // cabe en el pomodoro. La que no cabe se anula a mano.
                otro => Ok(vec![Effect::InterruptionLogged { task_id: otro.task_id(), kind }]),
            },

            Command::SkipBreak => match self.state.clone() {
                TimerState::Break { .. } if self.cfg.strict => Err(RuleViolation::BreakMandatory),
                TimerState::Break { kind, after_task, .. } => {
                    self.state = TimerState::Idle;
                    let mut fx = vec![Effect::BreakEnded { kind, at: now }];
                    if let Some(task_id) = after_task {
                        fx.push(Effect::SessionClosed {
                            task_id,
                            reason: SessionEnd::Idle,
                            at: now,
                        });
                    }
                    Ok(fx)
                }
                _ => Err(RuleViolation::NothingRunning),
            },
        }
    }

    fn abrir_foco(&mut self, task_id: i64, now: DateTime<Utc>) -> Vec<Effect> {
        let ends_at = now + Duration::minutes(self.cfg.focus_minutes as i64);
        self.state = TimerState::Focus { task_id, started_at: now, ends_at, overlearning: false };
        vec![
            Effect::SessionOpened { task_id },
            Effect::TaskStarted { task_id },
            Effect::FocusStarted { task_id, ends_at },
        ]
    }

    fn anular(
        &self,
        task_id: i64,
        started_at: DateTime<Utc>,
        reason: VoidReason,
        now: DateTime<Utc>,
    ) -> Effect {
        Effect::FocusVoided {
            task_id,
            reason,
            elapsed_secs: (now - started_at).num_seconds().max(0),
            at: now,
        }
    }

    fn duracion_descanso(&self, kind: BreakKind) -> Duration {
        Duration::minutes(match kind {
            BreakKind::Short => self.cfg.short_break_minutes as i64,
            BreakKind::Long => self.cfg.long_break_minutes as i64,
        })
    }
}
