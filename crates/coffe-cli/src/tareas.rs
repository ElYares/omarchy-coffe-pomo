//! Los comandos de gestión y cómo se pinta todo en la terminal.
//!
//! Estos hablan directamente con SQLite y no con el daemon: dar de alta una
//! tarea no necesita que el reloj esté corriendo, y sería absurdo que no se
//! pudiera apuntar nada con el daemon apagado.

use anyhow::{Context, Result};
use chrono::Utc;
use coffe_core::Db;
use coffe_core::config::Config;
use coffe_core::db::projects::{NuevoProyecto, Project};
use coffe_core::db::tasks::{AvisoEstimacion, FiltroTareas, NuevaTarea, revisar_estimacion};
use coffe_core::model::TaskState;
use coffe_ipc::{Phase, Snapshot};

use crate::{ListArgs, ProjectCmd, TaskCmd, parsear_prioridad};

// ---------------------------------------------------------------- proyectos

pub fn proyectos(db: &Db, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::Add { nombre, parent, repo } => {
            let parent_id = match parent {
                None => None,
                Some(p) => Some(resolver_proyecto(db, &p)?),
            };
            // Una ruta relativa aquí sería una bomba de relojería: se guarda
            // para compararla con el cwd de otra sesión.
            let repo = match repo {
                None => None,
                Some(r) => Some(
                    std::fs::canonicalize(&r)
                        .with_context(|| format!("no existe la carpeta {r}"))?
                        .display()
                        .to_string(),
                ),
            };

            let id = db.crear_proyecto(
                NuevoProyecto {
                    parent_id,
                    name: nombre.clone(),
                    repo_path: repo,
                    vault_path: None,
                },
                Utc::now(),
            )?;
            println!("[{id}] {}", db.ruta_proyecto(id)?);
        }

        ProjectCmd::List => {
            let todos = db.proyectos(false)?;
            if todos.is_empty() {
                println!("Todavía no hay proyectos. Empieza con:");
                println!("  coffe project add strapp");
                println!("  coffe project add tl-mas --parent strapp");
                return Ok(());
            }
            for p in &todos {
                println!("{}", linea_proyecto(&todos, p));
            }
        }
    }
    Ok(())
}

/// Sangra según la profundidad, para que el árbol se lea como árbol.
fn linea_proyecto(todos: &[Project], p: &Project) -> String {
    let mut nivel = 0;
    let mut actual = p.parent_id;
    while let Some(id) = actual {
        nivel += 1;
        actual = todos.iter().find(|x| x.id == id).and_then(|x| x.parent_id);
    }
    let sangria = "  ".repeat(nivel);
    let repo = p.repo_path.as_deref().map(|r| format!("  ← {r}")).unwrap_or_default();
    format!("{sangria}[{}] {}{repo}", p.id, p.name)
}

/// Acepta el id o la ruta de slugs (`personal/labs`). Los ids son cómodos para
/// los scripts y las rutas para las personas.
pub fn resolver_proyecto(db: &Db, referencia: &str) -> Result<i64> {
    if let Ok(id) = referencia.parse::<i64>() {
        return Ok(db.proyecto(id)?.id);
    }

    let buscado = referencia.trim_matches('/');
    let todos = db.proyectos(true)?;

    let coincidencias: Vec<&Project> =
        todos.iter().filter(|p| ruta_de_slugs(&todos, p).ends_with(buscado)).collect();

    match coincidencias.as_slice() {
        [uno] => Ok(uno.id),
        [] => anyhow::bail!("no encuentro el proyecto {referencia:?}"),
        varios => {
            let rutas: Vec<String> = varios.iter().map(|p| ruta_de_slugs(&todos, p)).collect();
            anyhow::bail!("{referencia:?} es ambiguo: {}", rutas.join(", "))
        }
    }
}

fn ruta_de_slugs(todos: &[Project], p: &Project) -> String {
    let mut partes = vec![p.slug.clone()];
    let mut actual = p.parent_id;
    while let Some(id) = actual {
        match todos.iter().find(|x| x.id == id) {
            Some(padre) => {
                partes.push(padre.slug.clone());
                actual = padre.parent_id;
            }
            None => break,
        }
    }
    partes.reverse();
    partes.join("/")
}

// ------------------------------------------------------------------ tareas

pub fn tareas(db: &Db, cfg: &Config, cmd: TaskCmd) -> Result<()> {
    match cmd {
        TaskCmd::Add { titulo, project, priority, due, estimate } => {
            let project_id = resolver_proyecto(db, &project)?;
            if let Some(d) = &due {
                validar_fecha(d)?;
            }

            let id = db.crear_tarea(
                NuevaTarea {
                    project_id,
                    title: titulo.clone(),
                    notes: None,
                    priority: parsear_prioridad(&priority)?,
                    estimate_pomodoros: estimate,
                    due_date: due,
                    vault_note: None,
                },
                Utc::now(),
            )?;

            println!("[{id}] {titulo}  ({})", db.ruta_proyecto(project_id)?);

            // Cirillo no impide guardarla, pero sí lo dice en voz alta.
            if let Some(aviso) = revisar_estimacion(estimate, cfg.pomodoro.max_pomodoros_per_task) {
                match aviso {
                    AvisoEstimacion::DemasiadoGrande { estimados, max } => {
                        println!("Aviso: {estimados} pomodoros es más de {max}. Pártela en dos.")
                    }
                    AvisoEstimacion::DemasiadoChica => {
                        println!("Aviso: menos de un pomodoro. Júntala con otra pequeña.")
                    }
                }
            }
        }

        TaskCmd::List(args) => listar(db, args)?,

        TaskCmd::Show { task } => mostrar(db, task)?,

        TaskCmd::Priority { task, priority } => {
            let p = parsear_prioridad(&priority)?;
            db.cambiar_prioridad(task, p)?;
            println!("[{task}] prioridad {}", p.etiqueta());
        }
    }
    Ok(())
}

fn listar(db: &Db, args: ListArgs) -> Result<()> {
    let project_id = match &args.project {
        None => None,
        Some(p) => Some(resolver_proyecto(db, p)?),
    };
    let states = match &args.state {
        None => None,
        Some(s) => Some(vec![s.parse::<TaskState>().map_err(|e| anyhow::anyhow!("{e}"))?]),
    };

    let filtro =
        FiltroTareas { project_id, incluir_descendientes: args.recursivo, states, priority: None };
    let encontradas = db.tareas(&filtro)?;

    if encontradas.is_empty() {
        println!("Nada por aquí.");
        return Ok(());
    }

    for t in &encontradas {
        let hechos = db.pomodoros_de_tarea(t.id)?;
        let avance = match t.estimate_pomodoros {
            Some(e) => format!("{hechos}/{e}"),
            None => format!("{hechos}"),
        };
        println!(
            "{:>4}  {:<9} {:<12} {:<7} {:<28} {}",
            t.id,
            marca_estado(t.state),
            t.due_date.as_deref().unwrap_or("—"),
            t.priority.etiqueta(),
            recortar(&t.title, 28),
            format_args!("[{avance}] {}", db.ruta_proyecto(t.project_id)?),
        );
    }
    Ok(())
}

/// Las tres medidas del tiempo, una debajo de otra. Puestas juntas es cuando
/// se ve lo que costó de verdad una tarea: cincuenta minutos de trabajo
/// repartidos en dos días y medio no son lo mismo que cincuenta minutos.
fn mostrar(db: &Db, task_id: i64) -> Result<()> {
    let t = db.tarea(task_id)?;
    let r = db.resumen_tarea(task_id)?;

    println!("[{}] {}", t.id, t.title);
    println!("  proyecto    : {}", db.ruta_proyecto(t.project_id)?);
    println!("  estado      : {}", marca_estado(t.state));
    println!("  prioridad   : {}", t.priority.etiqueta());
    if let Some(d) = &t.due_date {
        println!("  entrega     : {d}");
    }
    println!();

    let estimados = t.estimate_pomodoros.map(|e| format!(" de {e} estimados")).unwrap_or_default();
    println!("  pomodoros   : {}{estimados}", r.pomodoros_completados);
    if r.pomodoros_anulados > 0 {
        println!("  anulados    : {}", r.pomodoros_anulados);
    }
    println!("  efectivo    : {}", duracion(r.segundos_efectivos));
    println!("  dedicación  : {}", duracion(r.segundos_en_tramos));
    match r.segundos_calendario {
        Some(s) => println!("  calendario  : {}", duracion(s)),
        None => println!("  calendario  : (sin terminar)"),
    }
    println!("  pausas      : {}", r.veces_aparcada);
    println!(
        "  interrup.   : {} internas ('), {} externas (\")",
        r.interrupciones_internas, r.interrupciones_externas
    );
    Ok(())
}

/// Segundos a algo que se lee: `2 d 0 h 30 min`, `41 min`, `12 s`.
fn duracion(segundos: i64) -> String {
    if segundos <= 0 {
        return "—".to_string();
    }
    let dias = segundos / 86_400;
    let horas = (segundos % 86_400) / 3_600;
    let min = (segundos % 3_600) / 60;

    if dias > 0 {
        format!("{dias} d {horas} h {min} min")
    } else if horas > 0 {
        format!("{horas} h {min} min")
    } else if min > 0 {
        format!("{min} min")
    } else {
        format!("{segundos} s")
    }
}

fn marca_estado(s: TaskState) -> &'static str {
    match s {
        TaskState::Pending => "pendiente",
        TaskState::InProgress => "en curso",
        TaskState::Paused => "refri",
        TaskState::Done => "hecha",
        TaskState::Archived => "archivada",
    }
}

pub fn papelera(db: &Db, vaciar: bool) -> Result<()> {
    if vaciar {
        let n = db.vaciar_papelera(Utc::now())?;
        println!("Bote vaciado: {n} taza(s) fuera.");
        return Ok(());
    }

    let filtro = FiltroTareas { states: Some(vec![TaskState::Done]), ..Default::default() };
    let hechas = db.tareas(&filtro)?;
    if hechas.is_empty() {
        println!("El bote está vacío.");
        return Ok(());
    }
    for t in &hechas {
        println!("{:>4}  {}", t.id, t.title);
    }
    println!("\n{} taza(s). `coffe trash --empty` para vaciarlo.", hechas.len());
    Ok(())
}

fn validar_fecha(s: &str) -> Result<()> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("la fecha {s:?} no es AAAA-MM-DD"))?;
    Ok(())
}

// ------------------------------------------------------------------ pintado

/// Una línea con lo esencial. Es lo que se imprime tras cada comando.
pub fn una_linea(snap: &Snapshot) -> String {
    let que = match snap.phase {
        Phase::Idle => "Reloj parado".to_string(),
        Phase::Focus => format!("Pomodoro {}", snap.reloj()),
        Phase::Overlearning => format!("Repaso {}", snap.reloj()),
        Phase::ShortBreak => format!("Descanso {}", snap.reloj()),
        Phase::LongBreak => format!("Descanso largo {}", snap.reloj()),
    };
    match &snap.task {
        Some(t) => format!("{que} — {} ({})", t.title, t.project),
        None => que,
    }
}

/// El estado completo, para `coffe status`.
pub fn pintar_estado(snap: &Snapshot) -> String {
    let mut s = String::new();
    s.push_str(&format!("{}\n", una_linea(snap)));

    if let Some(t) = &snap.task {
        let avance = match t.estimate_pomodoros {
            Some(e) => format!("{}/{e} pomodoros", t.done_pomodoros),
            None => format!("{} pomodoros", t.done_pomodoros),
        };
        s.push_str(&format!(
            "  tarea       : [{}] {} — prioridad {}, {avance}\n",
            t.id,
            t.title,
            t.priority.etiqueta()
        ));
    }

    let faltan = snap.long_break_every.saturating_sub(snap.completed_since_long_break);
    s.push_str(&format!("  hoy         : {} pomodoro(s)\n", snap.pomodoros_hoy));
    s.push_str(&format!(
        "  ciclo       : {}/{} — {} para el descanso largo\n",
        snap.completed_since_long_break, snap.long_break_every, faltan
    ));
    s.push_str(&format!("  modo        : {}\n", if snap.strict { "estricto" } else { "flexible" }));
    if snap.en_papelera > 0 {
        s.push_str(&format!("  en el bote  : {} taza(s)\n", snap.en_papelera));
    }
    s
}

fn recortar(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let corto: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{corto}…")
}
