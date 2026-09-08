//! En qué se te fue el tiempo.
//!
//! Los números salen de `coffe_core::db::reportes`, que es lo que está probado.
//! Aquí solo se pintan, con una regla: **ningún número aparece solo**. Un total
//! sin su contexto se lee mal — cuarenta pomodoros en cuatro días no es lo mismo
//! que en veinte — y un porcentaje sobre tres casos no es un porcentaje.

use anyhow::Result;
use chrono::{Duration, Utc};
use coffe_core::Db;
use coffe_core::db::reportes::{Exportacion, Precision};

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
        if p.sin_estimar > 0 {
            println!("  Terminaste {} sin estimar: no hay con qué compararlas.", p.sin_estimar);
        }
        println!("  Sin eso no hay con qué comparar: estima antes de empezar.");
        return;
    }

    println!("  {} tarea(s): dijiste {} pomodoros y gastaste {}", p.tareas, p.estimados, p.reales);
    println!(
        "  te quedaste corto en {}, clavaste {}, sobró en {}",
        p.subestimadas, p.clavadas, p.sobreestimadas
    );
    // Lo que la cuenta NO mira. Sin esta línea, un factor sacado de dos tareas
    // mientras otras treinta se terminaron a ojo se lee como tu forma de
    // estimar, y es la de dos tareas.
    if p.sin_estimar > 0 {
        println!("  ({} terminada(s) sin estimar quedan fuera de esta cuenta)", p.sin_estimar);
    }

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
    // El texto lo arma `coffe_core`, que es quien lo tiene probado y quien se
    // lo da también a la ventana. Aquí solo se decide dónde va a parar.
    let que = match cmd {
        ExportCmd::Pomodoros { dias } => Exportacion::Pomodoros { dias },
        ExportCmd::Tareas => Exportacion::Tareas,
    };
    let (texto, _filas) = db.csv(que)?;
    print!("{texto}");
    Ok(())
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
