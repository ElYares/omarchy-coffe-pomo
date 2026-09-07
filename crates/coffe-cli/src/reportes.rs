//! En qué se te fue el tiempo.
//!
//! Los números salen de `coffe_core::db::reportes`, que es lo que está probado.
//! Aquí solo se pintan, con una regla: **ningún número aparece solo**. Un total
//! sin su contexto se lee mal — cuarenta pomodoros en cuatro días no es lo mismo
//! que en veinte — y un porcentaje sobre tres casos no es un porcentaje.

use anyhow::Result;
use chrono::{Duration, Utc};
use coffe_core::Db;
use coffe_core::db::reportes::Precision;
use coffe_core::db::tasks::FiltroTareas;
use coffe_core::model::TaskState;

use crate::ExportCmd;

/// Por debajo de esto, un porcentaje es una anécdota con decimales.
const MINIMO_PARA_HABLAR_DE_TENDENCIA: u32 = 5;

pub fn pintar(db: &Db, dias: u32) -> Result<()> {
    let hasta = Utc::now();
    let desde = hasta - Duration::days(dias.clamp(1, 3650) as i64);

    let r = db.resumen_periodo(desde, hasta)?;
    println!("Últimos {dias} días\n");

    if r.pomodoros_completados == 0 && r.pomodoros_anulados == 0 {
        println!("  Ni un pomodoro. Nada que contar.");
        return Ok(());
    }

    println!("  pomodoros   : {} en {} día(s)", r.pomodoros_completados, r.dias_con_trabajo);
    println!("  efectivo    : {}", duracion(r.segundos_efectivos));
    if r.dias_con_trabajo > 0 {
        let por_dia = r.pomodoros_completados as f64 / r.dias_con_trabajo as f64;
        println!("  por día     : {por_dia:.1} de media, los días que trabajaste");
    }
    if r.pomodoros_anulados > 0 {
        println!(
            "  tirados     : {} ({:.0}% de los que empezaste)",
            r.pomodoros_anulados,
            r.tasa_de_anulacion() * 100.0
        );
    }
    println!("  terminadas  : {} tarea(s)", r.tareas_terminadas);
    if r.interrupciones_internas + r.interrupciones_externas > 0 {
        println!(
            "  interrup.   : {} internas ('), {} externas (\")",
            r.interrupciones_internas, r.interrupciones_externas
        );
    }
    if r.segundos_con_claude > 0 {
        println!("  con Claude  : {}", duracion(r.segundos_con_claude));
    }

    let cargas = db.carga_por_proyecto(desde, hasta)?;
    if !cargas.is_empty() {
        println!("\nDónde se fue");
        let tope = cargas.iter().map(|c| c.segundos_efectivos).max().unwrap_or(1).max(1);
        for c in &cargas {
            println!(
                "  {:<34} {:>3} pom  {:>10}  {}",
                recortar(&c.ruta, 34),
                c.pomodoros,
                duracion(c.segundos_efectivos),
                barra(c.segundos_efectivos, tope)
            );
        }
    }

    pintar_precision(&db.precision_estimacion()?);
    Ok(())
}

fn pintar_precision(p: &Precision) {
    println!("\nTus estimaciones");

    if p.tareas == 0 {
        println!("  Todavía no hay ninguna tarea terminada CON estimación.");
        println!("  Sin eso no hay con qué comparar: estima antes de empezar.");
        return;
    }

    println!("  {} tarea(s): dijiste {} pomodoros y gastaste {}", p.tareas, p.estimados, p.reales);
    println!(
        "  te quedaste corto en {}, clavaste {}, sobró en {}",
        p.subestimadas, p.clavadas, p.sobreestimadas
    );

    let Some(factor) = p.factor() else { return };

    // Un factor sobre dos tareas es ruido con decimales. Se enseña igual, pero
    // diciendo que no es una tendencia: callarlo invitaría a creérselo.
    if p.tareas < MINIMO_PARA_HABLAR_DE_TENDENCIA {
        println!(
            "  (de momento ×{factor:.1}, pero con {} tarea(s) eso no es una tendencia)",
            p.tareas
        );
        return;
    }

    if factor > 1.15 {
        println!("  Multiplica por {factor:.1} lo que estimes: se te queda corto.");
    } else if factor < 0.85 {
        println!("  Estimas de más: gastas ×{factor:.1} de lo que dices.");
    } else {
        println!("  Van bien: ×{factor:.1}.");
    }
}

// ------------------------------------------------------------------- CSV

pub fn exportar(db: &Db, cmd: ExportCmd) -> Result<()> {
    match cmd {
        ExportCmd::Pomodoros { dias } => {
            let hasta = Utc::now();
            let desde = hasta - Duration::days(dias.clamp(1, 3650) as i64);
            println!("inicio,fin,resultado,motivo,segundos,estricto,tarea,proyecto");

            for f in db.pomodoros_del_periodo(desde, hasta)? {
                println!(
                    "{},{},{},{},{},{},{},{}",
                    f.started_at,
                    f.ended_at.unwrap_or_default(),
                    f.outcome.unwrap_or_default(),
                    f.void_reason.unwrap_or_default(),
                    f.planned_secs,
                    if f.strict { "si" } else { "no" },
                    csv(&f.titulo),
                    csv(&f.proyecto),
                );
            }
        }

        ExportCmd::Tareas => {
            println!(
                "id,titulo,proyecto,estado,prioridad,entrega,estimados,completados,anulados,\
                 seg_efectivos,seg_dedicacion,seg_calendario,pausas,interrup_internas,\
                 interrup_externas,seg_claude,nota"
            );
            // Todos los estados, archivadas incluidas: una exportación que se
            // deja fuera lo archivado pierde justo la historia vieja, que es
            // para lo que se exporta.
            let todos = vec![
                TaskState::Pending,
                TaskState::InProgress,
                TaskState::Paused,
                TaskState::Done,
                TaskState::Archived,
            ];
            for t in db.tareas(&FiltroTareas { states: Some(todos), ..Default::default() })? {
                let r = db.resumen_tarea(t.id)?;
                println!(
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    t.id,
                    csv(&t.title),
                    csv(&db.ruta_proyecto(t.project_id).unwrap_or_default()),
                    t.state.as_str(),
                    t.priority.as_str(),
                    t.due_date.unwrap_or_default(),
                    t.estimate_pomodoros.map(|e| e.to_string()).unwrap_or_default(),
                    r.pomodoros_completados,
                    r.pomodoros_anulados,
                    r.segundos_efectivos,
                    r.segundos_en_tramos,
                    r.segundos_calendario.map(|s| s.to_string()).unwrap_or_default(),
                    r.veces_aparcada,
                    r.interrupciones_internas,
                    r.interrupciones_externas,
                    r.segundos_con_claude,
                    csv(&t.vault_note.unwrap_or_default()),
                );
            }
        }
    }
    Ok(())
}

/// Un campo CSV. Los títulos llevan comas y comillas más a menudo de lo que
/// parece, y una sola sin escapar corre todas las columnas de esa fila.
fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ------------------------------------------------------------------ pinta

fn barra(valor: i64, tope: i64) -> String {
    let ancho = ((valor as f64 / tope as f64) * 18.0).round() as usize;
    "▉".repeat(ancho.max(usize::from(valor > 0)))
}

fn duracion(segundos: i64) -> String {
    if segundos <= 0 {
        return "—".to_string();
    }
    let h = segundos / 3600;
    let m = (segundos % 3600) / 60;
    if h > 0 { format!("{h} h {m:02} min") } else { format!("{m} min") }
}

fn recortar(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max - 1).collect::<String>())
}
