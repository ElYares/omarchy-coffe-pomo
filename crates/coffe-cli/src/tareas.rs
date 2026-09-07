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
use coffe_core::db::tasks::{
    AvisoEstimacion, CambiosTarea, FiltroTareas, NuevaTarea, revisar_estimacion,
};
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
            let repo = match repo {
                None => None,
                Some(r) => Some(canonica(&r)?),
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

        ProjectCmd::List { all } => {
            let todos = db.proyectos(all)?;
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

        ProjectCmd::Rename { proyecto, nombre } => {
            let id = resolver_proyecto(db, &proyecto)?;
            db.renombrar_proyecto(id, &nombre)?;
            println!("[{id}] {}", db.ruta_proyecto(id)?);
        }

        ProjectCmd::Move { proyecto, parent, root } => {
            let id = resolver_proyecto(db, &proyecto)?;
            let destino = match (&parent, root) {
                (Some(p), _) => Some(resolver_proyecto(db, p)?),
                (None, true) => None,
                (None, false) => anyhow::bail!("dime a dónde: --parent <proyecto> o --root"),
            };
            db.mover_proyecto(id, destino)?;
            println!("[{id}] {}", db.ruta_proyecto(id)?);
        }

        ProjectCmd::Repo { proyecto, ruta, clear } => {
            let id = resolver_proyecto(db, &proyecto)?;
            match (ruta, clear) {
                (Some(r), _) => {
                    let abs = canonica(&r)?;
                    db.fijar_repo(id, Some(&abs))?;
                    println!("[{id}] {} ← {abs}", db.ruta_proyecto(id)?);
                }
                (None, true) => {
                    db.fijar_repo(id, None)?;
                    println!("[{id}] {} — sin repo", db.ruta_proyecto(id)?);
                }
                (None, false) => anyhow::bail!("dime la ruta, o --clear para quitarla"),
            }
        }

        ProjectCmd::Archive { proyecto } => {
            let id = resolver_proyecto(db, &proyecto)?;
            let ruta = db.ruta_proyecto(id)?;
            let n = db.archivar_proyecto(id, true)?;
            println!("Archivado: {ruta} ({n} proyecto(s), contando lo que colgaba)");
        }

        ProjectCmd::Restore { proyecto } => {
            let id = resolver_proyecto(db, &proyecto)?;
            let n = db.archivar_proyecto(id, false)?;
            println!("De vuelta: {} ({n} proyecto(s))", db.ruta_proyecto(id)?);
        }

        ProjectCmd::Vault { proyecto, carpeta, clear } => {
            let id = resolver_proyecto(db, &proyecto)?;
            match (carpeta, clear) {
                (Some(c), _) => {
                    db.fijar_vault(id, Some(&c))?;
                    println!("[{id}] {} ← vault: {c}", db.ruta_proyecto(id)?);
                }
                (None, true) => {
                    db.fijar_vault(id, None)?;
                    println!("[{id}] {} — desligado del vault", db.ruta_proyecto(id)?);
                }
                (None, false) => anyhow::bail!("dime la carpeta, o --clear para desligarla"),
            }
        }

        ProjectCmd::Here => {
            let cwd = std::env::current_dir()?.display().to_string();
            match db.proyecto_por_ruta(&cwd)? {
                Some(p) => println!("[{}] {}", p.id, db.ruta_proyecto(p.id)?),
                None => {
                    println!("Este directorio no es de ningún proyecto.");
                    println!("  coffe project repo <proyecto> {cwd}");
                }
            }
        }

        ProjectCmd::Rm { proyecto, force } => {
            let id = resolver_proyecto(db, &proyecto)?;
            let ruta = db.ruta_proyecto(id)?;
            let (hijos, tareas) = db.contenido_proyecto(id)?;
            db.borrar_proyecto(id, force)?;
            if hijos > 0 || tareas > 0 {
                println!("Borrado: {ruta} — con {hijos} subproyecto(s) y {tareas} tarea(s)");
            } else {
                println!("Borrado: {ruta}");
            }
        }
    }
    Ok(())
}

/// Una ruta relativa guardada aquí sería una bomba de relojería: se compara
/// contra el cwd de otra sesión, que puede estar en cualquier sitio.
fn canonica(r: &str) -> Result<String> {
    Ok(std::fs::canonicalize(r)
        .with_context(|| format!("no existe la carpeta {r}"))?
        .display()
        .to_string())
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
    let archivado = if p.archived { "  (archivado)" } else { "" };
    format!("{sangria}[{}] {}{repo}{archivado}", p.id, p.name)
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
            let project_id = match &project {
                Some(p) => resolver_proyecto(db, p)?,
                // Sin `--project`, el del directorio. Apuntar una tarea del
                // repo en el que estás no debería obligarte a teclear su ruta.
                None => {
                    let cwd = std::env::current_dir()?.display().to_string();
                    db.proyecto_por_ruta(&cwd)?
                        .map(|p| p.id)
                        .context("este directorio no es de ningún proyecto: usa --project")?
                }
            };
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
                    vault_id: None,
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

        TaskCmd::Edit(args) => {
            if let Some(d) = &args.due {
                validar_fecha(d)?;
            }
            let cambios = CambiosTarea {
                title: args.title,
                notes: args.notes.map(Some),
                due_date: if args.clear_due { Some(None) } else { args.due.map(Some) },
                estimate_pomodoros: if args.clear_estimate {
                    Some(None)
                } else {
                    args.estimate.map(Some)
                },
                vault_note: None,
            };
            if cambios.vacio() {
                anyhow::bail!("no me dijiste qué cambiar (--title, --due, --estimate, --notes)");
            }
            db.editar_tarea(args.task, &cambios)?;

            let t = db.tarea(args.task)?;
            println!("[{}] {}", t.id, t.title);
            println!("  entrega    : {}", t.due_date.as_deref().unwrap_or("—"));
            match t.estimate_pomodoros {
                Some(e) => println!("  estimación : {e} pomodoros"),
                None => println!("  estimación : —"),
            }
            if let Some(aviso) =
                revisar_estimacion(t.estimate_pomodoros, cfg.pomodoro.max_pomodoros_per_task)
            {
                println!("{}", texto_aviso(aviso));
            }
        }

        TaskCmd::Move { task, project } => {
            let destino = resolver_proyecto(db, &project)?;
            db.mover_tarea(task, destino)?;
            println!("[{task}] {} → {}", db.tarea(task)?.title, db.ruta_proyecto(destino)?);
        }

        TaskCmd::Reopen { task } => {
            db.reabrir(task)?;
            println!("[{task}] {} — de vuelta a pendiente", db.tarea(task)?.title);
        }

        TaskCmd::Rm { task, force } => {
            let t = db.tarea(task)?;
            let r = db.resumen_tarea(task)?;
            db.borrar_tarea(task, force)?;
            if r.pomodoros_completados > 0 || r.pomodoros_anulados > 0 {
                println!(
                    "Borrada: {} — con {} pomodoro(s) de historial",
                    t.title,
                    r.pomodoros_completados + r.pomodoros_anulados
                );
            } else {
                println!("Borrada: {}", t.title);
            }
        }
    }
    Ok(())
}

fn texto_aviso(aviso: AvisoEstimacion) -> String {
    match aviso {
        AvisoEstimacion::DemasiadoGrande { estimados, max } => {
            format!("Aviso: {estimados} pomodoros es más de {max}. Pártela en dos.")
        }
        AvisoEstimacion::DemasiadoChica => {
            "Aviso: menos de un pomodoro. Júntala con otra pequeña.".to_string()
        }
    }
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
    if r.segundos_con_claude > 0 {
        // Se compara contra la dedicación y no contra el tiempo efectivo: la
        // dedicación es el rato en que de verdad se estuvo en la tarea.
        let pct =
            (r.segundos_con_claude as f64 / r.segundos_en_tramos.max(1) as f64 * 100.0).round();
        println!("  con Claude  : {} ({pct}%)", duracion(r.segundos_con_claude));
    }
    if let Some(nota) = &t.vault_note {
        println!("  nota        : {nota}");
    }
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
