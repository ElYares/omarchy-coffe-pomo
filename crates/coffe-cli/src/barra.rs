//! El módulo de waybar.
//!
//! `coffe bar` imprime una línea de JSON y sale; `coffe bar --watch` se queda
//! suscrito e imprime una por segundo mientras el reloj corra. La segunda es la
//! que usa la barra: con `interval` waybar tendría que arrancar un proceso cada
//! segundo solo para mover una cuenta atrás.
//!
//! `coffe click` es para los botones del ratón. Es igual que los comandos
//! normales salvo en que el resultado se cuenta por notificación: desde la
//! barra no hay stdout que leer, y un clic que no hace nada visible parece un
//! clic que no funcionó.

use anyhow::Result;
use coffe_core::paths;
use coffe_ipc::client::{self, Client, IpcError};
use coffe_ipc::{Phase, Request, Snapshot};
use serde::Serialize;

/// La taza de Phosphor Fill (`coffee`). La barra ya usa esa fuente para los
/// iconos de los espacios de trabajo.
const TAZA: char = '\u{e1c2}';

/// Lo que waybar espera de un módulo `custom` con `return-type: json`.
#[derive(Serialize)]
struct Modulo {
    text: String,
    tooltip: String,
    class: String,
    percentage: u8,
}

pub fn imprimir(watch: bool) -> Result<()> {
    if !watch {
        return linea(&pintar(consultar()));
    }

    // En modo continuo, que el daemon no esté no es motivo para morir: se
    // dibuja el módulo apagado y se reintenta. Waybar volverá a lanzarnos.
    let mut cliente = match Client::connect(&paths::socket()) {
        Ok(c) => c,
        Err(e) => return linea(&pintar(Err(e))),
    };

    // La respuesta a `subscribe` ES el primer snapshot, y hay que pintarlo: con
    // el reloj parado el daemon no empuja nada, asi que si se descarta, el
    // modulo se queda mudo hasta que alguien arranque un pomodoro.
    match cliente.send(&Request::Subscribe) {
        Ok(snap) => linea(&pintar(Ok(snap)))?,
        Err(e) => {
            linea(&pintar(Err(e)))?;
            return Ok(());
        }
    }

    loop {
        match cliente.next_snapshot() {
            Ok(Some(snap)) => linea(&pintar(Ok(snap)))?,
            // El daemon cerró: se apaga el módulo y se sale para que waybar
            // nos relance según su `restart-interval`.
            Ok(None) => {
                let apagado = IpcError::SinDaemon { ruta: paths::socket().display().to_string() };
                linea(&pintar(Err(apagado)))?;
                return Ok(());
            }
            Err(e) => {
                linea(&pintar(Err(e)))?;
                return Ok(());
            }
        }
    }
}

/// Waybar lee por líneas y no espera: cada una hay que empujarla o se queda en
/// el búfer hasta que haya 4 KB, que con un JSON por segundo son minutos.
fn linea(m: &Modulo) -> Result<()> {
    use std::io::Write;
    let mut salida = std::io::stdout().lock();
    writeln!(salida, "{}", serde_json::to_string(m)?)?;
    salida.flush()?;
    Ok(())
}

fn consultar() -> Result<Snapshot, IpcError> {
    client::ask(&paths::socket(), &Request::Status)
}

fn pintar(estado: Result<Snapshot, IpcError>) -> Modulo {
    let snap = match estado {
        Ok(s) => s,
        // Texto vacío esconde el módulo: con el reloj apagado, la barra no
        // tiene por qué enseñar un hueco.
        Err(e) => {
            return Modulo {
                text: String::new(),
                tooltip: escapar(&e.to_string()),
                class: "off".into(),
                percentage: 0,
            };
        }
    };

    let icono = format!("<span font='Phosphor-Fill 10'>{TAZA}</span>");
    let text = match snap.phase {
        // Parado se enseña solo la taza: sigue siendo el sitio donde pulsar.
        Phase::Idle => icono,
        _ => format!("{icono} {}", snap.reloj()),
    };

    Modulo {
        text,
        tooltip: tooltip(&snap),
        class: clase(snap.phase).into(),
        // En foco la taza se vacía; en descanso se llena. Es el mismo número
        // que moverá el dibujo de la ventana.
        percentage: nivel(&snap),
    }
}

fn clase(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "idle",
        Phase::Focus => "focus",
        Phase::Overlearning => "overlearning",
        Phase::ShortBreak => "break",
        Phase::LongBreak => "break-long",
    }
}

/// Cuánto café queda en la taza, de 0 a 100.
fn nivel(snap: &Snapshot) -> u8 {
    let fraccion = match snap.phase {
        Phase::Idle => 1.0,
        Phase::Focus | Phase::Overlearning => 1.0 - snap.progress,
        Phase::ShortBreak | Phase::LongBreak => snap.progress,
    };
    (fraccion * 100.0).round().clamp(0.0, 100.0) as u8
}

fn tooltip(snap: &Snapshot) -> String {
    let mut l: Vec<String> = Vec::new();

    l.push(match snap.phase {
        Phase::Idle => "Reloj parado".into(),
        Phase::Focus => format!("Pomodoro · {}", snap.reloj()),
        Phase::Overlearning => format!("Repaso · {} — la tarea ya está hecha", snap.reloj()),
        Phase::ShortBreak => format!("Descanso · {}", snap.reloj()),
        Phase::LongBreak => format!("Descanso largo · {}", snap.reloj()),
    });

    if let Some(t) = &snap.task {
        let avance = match t.estimate_pomodoros {
            Some(e) => format!("{}/{e}", t.done_pomodoros),
            None => t.done_pomodoros.to_string(),
        };
        l.push(String::new());
        l.push(escapar(&t.title));
        l.push(format!(
            "{} — prioridad {} — {avance} pomodoros",
            escapar(&t.project),
            t.priority.etiqueta()
        ));
    }

    l.push(String::new());
    l.push(format!("{} pomodoro(s) hoy", snap.pomodoros_hoy));

    let faltan = snap.long_break_every.saturating_sub(snap.completed_since_long_break);
    if faltan > 0 {
        l.push(format!("{faltan} para el descanso largo"));
    }
    if snap.en_papelera > 0 {
        l.push(format!("{} taza(s) en el bote", snap.en_papelera));
    }
    if !snap.strict {
        l.push("modo flexible: estos pomodoros no son canónicos".into());
    }

    l.join("\n")
}

/// Waybar interpreta el texto como marcado de Pango. Un título de tarea con un
/// `&` lo rompería entero.
fn escapar(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ------------------------------------------------------------------- clics

pub fn clic(accion: &str) -> Result<()> {
    let resultado = match accion {
        "toggle" => alternar(),
        "resume" => reanudar(),
        "pause" => client::ask(&paths::socket(), &Request::Pause).map(|_| "Al refri".to_string()),
        "void" => client::ask(
            &paths::socket(),
            &Request::Void { reason: coffe_core::VoidReason::Abandoned },
        )
        .map(|_| "Pomodoro tirado".to_string()),
        "done" => client::ask(&paths::socket(), &Request::Done).map(|_| "Tarea hecha".to_string()),
        "interrupt" => client::ask(
            &paths::socket(),
            &Request::Interrupt { kind: coffe_core::InterruptionKind::External, note: None },
        )
        .map(|_| "Interrupción apuntada (\") — sigue donde estabas".to_string()),
        otro => anyhow::bail!(
            "acción desconocida: {otro:?} (toggle, resume, pause, void, done, interrupt)"
        ),
    };

    match resultado {
        Ok(mensaje) => notificar("coffe", &mensaje),
        // Una regla del método rechazando algo no es un fallo: es la respuesta,
        // y desde la barra hay que poder verla.
        Err(e) => notificar("coffe", &e.to_string()),
    }
    Ok(())
}

/// La tecla única: si hay reloj corriendo, al refri; si no, saca del refri lo
/// último que se guardó. Es lo que se quiere el 90% de las veces, y evita
/// gastar dos atajos en una sola idea.
fn alternar() -> Result<String, IpcError> {
    let ahora = client::ask(&paths::socket(), &Request::Status)?;
    if ahora.phase == Phase::Idle {
        reanudar()
    } else {
        client::ask(&paths::socket(), &Request::Pause).map(|_| match ahora.task {
            Some(t) => format!("Al refri: {}", t.title),
            None => "Al refri".to_string(),
        })
    }
}

fn reanudar() -> Result<String, IpcError> {
    let db =
        coffe_core::Db::open(&paths::database()).map_err(|e| IpcError::Rechazado(e.to_string()))?;
    let tarea = db
        .ultima_aparcada()
        .map_err(|e| IpcError::Rechazado(e.to_string()))?
        .ok_or_else(|| IpcError::Rechazado("no hay nada en el refri".into()))?;

    client::ask(&paths::socket(), &Request::Start { task_id: tarea.id })
        .map(|_| format!("En marcha: {}", tarea.title))
}

fn notificar(titulo: &str, cuerpo: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "coffe", "-i", "coffee", titulo, cuerpo])
        .spawn();
}
