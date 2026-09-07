//! `coffe` — la línea de comandos y, con `coffe daemon`, el propio reloj.
//!
//! Es un solo binario a propósito: waybar lo invoca una y otra vez para pintar
//! la barra, y cada milisegundo de arranque se nota ahí.
//!
//! Los comandos se reparten en dos grupos según a quién le hablan. Los del
//! reloj (`start`, `pause`, `done`...) van por el socket, porque el estado lo
//! tiene el daemon. Los de gestión (`project`, `task`) van directos a SQLite,
//! y así siguen funcionando con el daemon apagado.

mod daemon;
mod tareas;
mod vista;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use coffe_core::model::{InterruptionKind, Priority, VoidReason};
use coffe_core::{Config, Db, paths};
use coffe_ipc::{Phase, Request, Snapshot, client};

#[derive(Parser)]
#[command(name = "coffe", version, about = "Pomodoro de café para Omarchy")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Arranca el reloj. Normalmente lo lanza systemd, no tú.
    Daemon,

    /// Cómo va todo.
    Status {
        /// Saca el snapshot entero en JSON.
        #[arg(long)]
        json: bool,
    },

    /// Arranca un pomodoro sobre una tarea.
    Start { task: i64 },

    /// Al refri: anula el pomodoro y aparca la tarea.
    Pause,

    /// Saca del refri la última tarea aparcada, o la que se le diga.
    Resume { task: Option<i64> },

    /// Tira el pomodoro en curso sin tocar la tarea.
    Void {
        /// Por qué se tira.
        #[arg(long, default_value = "abandoned")]
        reason: String,
    },

    /// Cambia de tarea en caliente.
    Switch { task: i64 },

    /// Da por terminada la tarea en curso.
    Done,

    /// Apunta una interrupción sin cortar el pomodoro.
    Interrupt {
        /// Externa: te interrumpió alguien. Por defecto es interna.
        #[arg(short, long)]
        externa: bool,
    },

    /// Corta el descanso. Solo fuera del modo estricto.
    SkipBreak,

    /// Proyectos.
    #[command(subcommand)]
    Project(ProjectCmd),

    /// Tareas.
    #[command(subcommand)]
    Task(TaskCmd),

    /// Las tazas del bote.
    Trash(TrashArgs),

    /// Dónde vive cada cosa y qué configuración está en uso.
    Paths,
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Da de alta un proyecto. Con `--parent` cuelga de otro.
    Add {
        nombre: String,
        /// El padre: su id o su ruta, como `personal/labs`.
        #[arg(long)]
        parent: Option<String>,
        /// La carpeta del repo, para reconocer el proyecto por el directorio.
        #[arg(long)]
        repo: Option<String>,
    },
    /// El árbol entero.
    List,
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Da de alta una tarea.
    Add {
        titulo: String,
        /// El proyecto: su id o su ruta, como `clientes/nutricore`.
        #[arg(long, short)]
        project: String,
        /// alta, media o baja.
        #[arg(long, short = 'P', default_value = "media")]
        priority: String,
        /// Fecha de entrega, `AAAA-MM-DD`.
        #[arg(long, short)]
        due: Option<String>,
        /// Cuántos pomodoros crees que lleva.
        #[arg(long, short)]
        estimate: Option<u32>,
    },
    /// Lo que hay.
    List(ListArgs),
    /// Los tiempos de una tarea: lo efectivo, lo de calendario y las pausas.
    Show { task: i64 },
    /// Cambia la prioridad. Solo si la tarea no ha arrancado.
    Priority { task: i64, priority: String },
}

#[derive(Args)]
struct ListArgs {
    /// Filtra por proyecto: id o ruta.
    #[arg(long, short)]
    project: Option<String>,
    /// Incluye los subproyectos.
    #[arg(long, short)]
    recursivo: bool,
    /// Filtra por estado.
    #[arg(long, short)]
    state: Option<String>,
}

#[derive(Args)]
struct TrashArgs {
    /// Vacía el bote: archiva todas las terminadas.
    #[arg(long)]
    empty: bool,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Daemon => arrancar_daemon(),

        Cmd::Status { json } => {
            let snap = pedir(&Request::Status)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&snap)?);
            } else {
                print!("{}", tareas::pintar_estado(&snap));
            }
            Ok(())
        }

        Cmd::Start { task } => contar(pedir(&Request::Start { task_id: task })?),

        Cmd::Pause => {
            // Quién se fue al refri hay que preguntarlo ANTES: después de
            // aparcarla, el snapshot ya no la lleva colgando.
            let antes = pedir(&Request::Status)?.task;
            let snap = pedir(&Request::Pause)?;
            if let Some(t) = antes {
                println!("Al refri: {} ({}).", t.title, t.project);
            }
            contar(snap)
        }
        Cmd::Switch { task } => contar(pedir(&Request::Switch { task_id: task })?),
        Cmd::Done => contar(pedir(&Request::Done)?),
        Cmd::SkipBreak => contar(pedir(&Request::SkipBreak)?),

        Cmd::Resume { task } => {
            let id = match task {
                Some(id) => id,
                None => {
                    let db = abrir_db()?;
                    db.ultima_aparcada()?.context("no hay ninguna tarea en el refri")?.id
                }
            };
            contar(pedir(&Request::Start { task_id: id })?)
        }

        Cmd::Void { reason } => {
            let reason = parsear_motivo(&reason)?;
            contar(pedir(&Request::Void { reason })?)
        }

        Cmd::Interrupt { externa } => {
            let kind =
                if externa { InterruptionKind::External } else { InterruptionKind::Internal };
            let snap = pedir(&Request::Interrupt { kind, note: None })?;
            println!("Apuntada {} — sigue donde estabas.", kind.marca());
            contar(snap)
        }

        Cmd::Project(cmd) => tareas::proyectos(&abrir_db()?, cmd),
        Cmd::Task(cmd) => tareas::tareas(&abrir_db()?, &config()?, cmd),
        Cmd::Trash(args) => tareas::papelera(&abrir_db()?, args.empty),

        Cmd::Paths => {
            let cfg_path = paths::config();
            let cfg = Config::load(&cfg_path)?;

            println!("base de datos : {}", paths::database().display());
            println!("configuración : {}", cfg_path.display());
            println!("socket        : {}", paths::socket().display());
            println!("tema omarchy  : {}", paths::omarchy_colors().display());
            println!();
            println!(
                "pomodoro      : {}/{} min, descanso largo de {} cada {}",
                cfg.pomodoro.focus_minutes,
                cfg.pomodoro.short_break_minutes,
                cfg.pomodoro.long_break_minutes,
                cfg.pomodoro.long_break_every,
            );
            println!(
                "modo          : {}",
                if cfg.pomodoro.strict { "estricto" } else { "flexible" }
            );
            Ok(())
        }
    }
}

/// El daemon es lo único que necesita runtime asíncrono, así que se monta aquí
/// y no en `main`: el resto de comandos no debe pagarlo.
fn arrancar_daemon() -> Result<()> {
    let cfg = config()?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async { daemon::Daemon::nuevo(cfg)?.correr().await })
}

fn pedir(req: &Request) -> Result<Snapshot> {
    Ok(client::ask(&paths::socket(), req)?)
}

/// Tras un comando del reloj, decir en una línea cómo quedó la cosa.
fn contar(snap: Snapshot) -> Result<()> {
    println!("{}", tareas::una_linea(&snap));
    if snap.phase == Phase::Overlearning {
        println!("El pomodoro sigue: usa lo que queda para repasar lo que hiciste.");
    }
    Ok(())
}

fn abrir_db() -> Result<Db> {
    Db::open(&paths::database()).context("no pude abrir la base")
}

fn config() -> Result<Config> {
    Ok(Config::load(&paths::config())?)
}

fn parsear_motivo(s: &str) -> Result<VoidReason> {
    Ok(match s.trim().to_ascii_lowercase().as_str() {
        "abandoned" | "abandonado" => VoidReason::Abandoned,
        "interrupted" | "interrumpido" => VoidReason::Interrupted,
        "switched" | "cambio" => VoidReason::Switched,
        otro => anyhow::bail!("motivo desconocido: {otro:?} (abandonado, interrumpido, cambio)"),
    })
}

pub fn parsear_prioridad(s: &str) -> Result<Priority> {
    s.parse().map_err(|e| anyhow::anyhow!("{e}"))
}
