//! El puente entre la máquina y la base: aplica un comando y escribe lo que la
//! máquina dice que pasó.
//!
//! Está aquí y no en el daemon porque no tiene nada de asíncrono ni de
//! escritorio, y porque así se puede probar entero contra una base en memoria.

use crate::config::Config;
use crate::db::Db;
use crate::error::CoffeError;
use crate::machine::{Command, Effect, Machine, SessionEnd, TimerState};
use crate::model::{InterruptionKind, VoidReason};
use chrono::{DateTime, Duration, Utc};

pub struct Service {
    db: Db,
    machine: Machine,
    cfg: Config,
}

impl Service {
    /// Levanta el servicio recuperando lo que hubiera quedado a medias.
    /// Devuelve los efectos de esa recuperación, para poder avisar de que un
    /// pomodoro se perdió en vez de tragárselo en silencio.
    pub fn arrancar(
        db: Db,
        cfg: Config,
        now: DateTime<Utc>,
    ) -> Result<(Self, Vec<Effect>), CoffeError> {
        let (estado, contador) = db.leer_timer()?.unwrap_or((TimerState::Idle, 0));
        let machine = Machine::restore(cfg.pomodoro.clone(), estado, contador);
        let mut svc = Self { db, machine, cfg };

        // Un tick pone el reloj al día: si el foco venció mientras el daemon
        // no estaba, la propia máquina decide si sonó o si se perdió.
        let fx = svc.ejecutar(Command::Tick, now)?;

        // Y lo que quedara abierto de una caída sucia se cierra ahora, no
        // dentro de tres días cuando alguien mire el reporte.
        if svc.machine.state().is_idle() {
            svc.db.cerrar_tramos_huerfanos(now)?;
            if let Some((_, _, started)) = svc.db.pomodoro_abierto()? {
                svc.db.anular_pomodoro(started, VoidReason::DaemonLost)?;
            }
        }

        Ok((svc, fx))
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn state(&self) -> &TimerState {
        self.machine.state()
    }

    /// Cuándo hay que volver a mirar el reloj, si hay algo que mirar.
    pub fn proximo_vencimiento(&self) -> Option<DateTime<Utc>> {
        self.machine.state().ends_at()
    }

    /// Aplica un comando y persiste sus consecuencias.
    pub fn ejecutar(
        &mut self,
        cmd: Command,
        now: DateTime<Utc>,
    ) -> Result<Vec<Effect>, CoffeError> {
        // Una tarea que no existe se rechaza ANTES de tocar la máquina. Si se
        // deja pasar, el reloj arranca en memoria y la escritura muere contra
        // la clave foránea: el usuario ve "FOREIGN KEY constraint failed" y el
        // estado queda con un foco abierto que no tiene pomodoro detrás.
        if let Command::Start { task_id } | Command::Switch { task_id } = cmd {
            self.db.tarea(task_id)?;
        }

        let fx = self.machine.apply(cmd, now)?;

        for efecto in &fx {
            self.escribir(efecto, now)?;
        }

        // El estado se guarda siempre, aunque no haya efectos: un `Tick` que
        // no cambia nada tampoco cuesta nada de escribir, y así el reloj de la
        // base nunca va por detrás del de memoria.
        self.db.guardar_timer(
            self.machine.state(),
            self.machine.completed_since_long_break(),
            now,
        )?;

        Ok(fx)
    }

    /// Cada efecto es exactamente una escritura. Si esto crece a un `match` con
    /// lógica dentro, es señal de que le falta un efecto a la máquina.
    fn escribir(&self, efecto: &Effect, now: DateTime<Utc>) -> Result<(), CoffeError> {
        match efecto {
            Effect::FocusStarted { task_id, ends_at } => {
                let planeados = (*ends_at - now).num_seconds().max(0);
                self.db.abrir_pomodoro(*task_id, now, planeados, self.cfg.pomodoro.strict)?;
            }
            Effect::FocusCompleted { at, .. } => {
                self.db.completar_pomodoro(*at)?;
            }
            Effect::FocusVoided { reason, at, .. } => {
                self.db.anular_pomodoro(*at, *reason)?;
            }
            // Los descansos no se guardan: no hay nada que reportar de ellos
            // que no se deduzca de los pomodoros que los abrieron.
            Effect::BreakStarted { .. } | Effect::BreakEnded { .. } => {}

            Effect::TaskStarted { task_id } => self.db.marcar_en_curso(*task_id, now)?,
            Effect::TaskParked { task_id } => self.db.aparcar(*task_id)?,
            Effect::TaskCompleted { task_id } => self.db.completar(*task_id, now)?,

            Effect::SessionOpened { task_id } => self.db.abrir_tramo(*task_id, now)?,
            Effect::SessionClosed { task_id, reason, at } => {
                self.db.cerrar_tramo(*task_id, *at, *reason)?
            }

            Effect::InterruptionLogged { task_id, kind } => {
                let pomodoro = self.db.pomodoro_abierto()?.map(|(id, _, _)| id);
                self.db.registrar_interrupcion(*task_id, pomodoro, *kind, now, None)?;
            }
        }
        Ok(())
    }

    // -- atajos que la CLI y el daemon usan tal cual --

    pub fn start(&mut self, task_id: i64, now: DateTime<Utc>) -> Result<Vec<Effect>, CoffeError> {
        self.ejecutar(Command::Start { task_id }, now)
    }

    pub fn pause(&mut self, now: DateTime<Utc>) -> Result<Vec<Effect>, CoffeError> {
        self.ejecutar(Command::Pause, now)
    }

    pub fn done(&mut self, now: DateTime<Utc>) -> Result<Vec<Effect>, CoffeError> {
        self.ejecutar(Command::Done, now)
    }

    pub fn interrupt(
        &mut self,
        kind: InterruptionKind,
        now: DateTime<Utc>,
    ) -> Result<Vec<Effect>, CoffeError> {
        self.ejecutar(Command::Interrupt { kind }, now)
    }

    /// Cuánto dura el foco configurado. Lo usa la barra para dibujar la taza
    /// cuando el reloj está parado.
    pub fn duracion_foco(&self) -> Duration {
        Duration::minutes(self.cfg.pomodoro.focus_minutes as i64)
    }
}

/// Un efecto que merece una notificación de escritorio, con su texto ya hecho.
pub fn aviso(efecto: &Effect) -> Option<(&'static str, String)> {
    match efecto {
        Effect::FocusCompleted { .. } => Some((
            "Pomodoro terminado",
            "Se acabó. Levántate: el descanso es parte del método.".into(),
        )),
        Effect::BreakEnded { .. } => {
            Some(("Descanso terminado", "Cuando quieras, siguiente pomodoro.".into()))
        }
        Effect::FocusVoided { reason: VoidReason::DaemonLost, .. } => Some((
            "Pomodoro perdido",
            "Sonó mientras el equipo estaba dormido, así que no cuenta.".into(),
        )),
        _ => None,
    }
}

/// Si un efecto cambia lo que se ve en la barra.
pub fn cambia_la_barra(efecto: &Effect) -> bool {
    !matches!(efecto, Effect::InterruptionLogged { .. } | Effect::SessionOpened { .. })
}

/// Los tramos que la recuperación cerró de mala manera, para avisar.
pub fn hubo_recuperacion(fx: &[Effect]) -> bool {
    fx.iter().any(|e| matches!(e, Effect::SessionClosed { reason: SessionEnd::Recovered, .. }))
}
