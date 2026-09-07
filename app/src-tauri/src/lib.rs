//! La ventana de coffe.
//!
//! No tiene reloj propio: todo lo que enseña se lo pregunta al daemon por el
//! mismo socket que usa la CLI. Cerrar la ventana no para un pomodoro, y abrir
//! dos ventanas no crea dos relojes.

mod tema;

use anyhow::Result;
use coffe_core::db::projects::Project;
use coffe_core::db::tasks::{FiltroTareas, Task};
use coffe_core::model::InterruptionKind;
use coffe_core::{Db, VoidReason, paths};
use coffe_ipc::client::{self, Client};
use coffe_ipc::{Request, Snapshot};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// Cuánto se espera antes de volver a intentar hablar con el daemon.
const REINTENTO: Duration = Duration::from_secs(2);

/// Cada cuánto se mira si cambió el tema de Omarchy. Es un `stat` de un archivo
/// de diez bytes; sale más barato que instalar un hook y depender de que esté.
const OJO_AL_TEMA: Duration = Duration::from_secs(2);

struct Estado {
    db: Mutex<Db>,
}

// ------------------------------------------------------------------ vistas

/// Un proyecto con su ruta ya resuelta: la ventana no debería tener que
/// recorrer el árbol para escribir "clientes / nutricore".
#[derive(Debug, Clone, Serialize)]
struct ProyectoVista {
    #[serde(flatten)]
    proyecto: Project,
    ruta: String,
    nivel: usize,
}

#[derive(Debug, Clone, Serialize)]
struct TareaVista {
    #[serde(flatten)]
    tarea: Task,
    proyecto: String,
    pomodoros: u32,
}

/// Lo que la ventana puede pedirle al reloj. Es un espejo de `Request`, pero
/// con nombres que el frontend pueda mandar como cadena.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum Orden {
    Start { task_id: i64 },
    Pause,
    Void,
    Switch { task_id: i64 },
    Done,
    Interrupt { externa: bool },
    SkipBreak,
}

impl From<Orden> for Request {
    fn from(o: Orden) -> Self {
        match o {
            Orden::Start { task_id } => Request::Start { task_id },
            Orden::Pause => Request::Pause,
            Orden::Void => Request::Void { reason: VoidReason::Abandoned },
            Orden::Switch { task_id } => Request::Switch { task_id },
            Orden::Done => Request::Done,
            Orden::Interrupt { externa } => Request::Interrupt {
                kind: if externa { InterruptionKind::External } else { InterruptionKind::Internal },
                note: None,
            },
            Orden::SkipBreak => Request::SkipBreak,
        }
    }
}

// ---------------------------------------------------------------- comandos

#[tauri::command]
fn estado() -> Result<Snapshot, String> {
    client::ask(&paths::socket(), &Request::Status).map_err(|e| e.to_string())
}

#[tauri::command]
fn mandar(orden: Orden) -> Result<Snapshot, String> {
    client::ask(&paths::socket(), &orden.into()).map_err(|e| e.to_string())
}

#[tauri::command]
fn tema() -> tema::Tema {
    tema::leer()
}

#[tauri::command]
fn proyectos(estado: State<'_, Estado>) -> Result<Vec<ProyectoVista>, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let todos = db.proyectos(false).map_err(|e| e.to_string())?;

    Ok(todos
        .iter()
        .map(|p| ProyectoVista {
            ruta: db.ruta_proyecto(p.id).unwrap_or_else(|_| p.name.clone()),
            nivel: profundidad(&todos, p),
            proyecto: p.clone(),
        })
        .collect())
}

#[tauri::command]
fn tareas(estado: State<'_, Estado>) -> Result<Vec<TareaVista>, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let encontradas = db.tareas(&FiltroTareas::default()).map_err(|e| e.to_string())?;

    Ok(encontradas
        .into_iter()
        .map(|t| TareaVista {
            proyecto: db.ruta_proyecto(t.project_id).unwrap_or_default(),
            pomodoros: db.pomodoros_de_tarea(t.id).unwrap_or(0),
            tarea: t,
        })
        .collect())
}

fn profundidad(todos: &[Project], p: &Project) -> usize {
    let mut n = 0;
    let mut actual = p.parent_id;
    while let Some(id) = actual {
        n += 1;
        actual = todos.iter().find(|x| x.id == id).and_then(|x| x.parent_id);
    }
    n
}

// ------------------------------------------------------------------- hilos

/// Se queda suscrito al daemon y reenvía cada snapshot al frontend. Si el
/// daemon no está, avisa y sigue reintentando: la ventana tiene que poder estar
/// abierta con el reloj apagado sin quedarse en blanco.
fn seguir_al_reloj(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            match suscribirse(&app) {
                Ok(()) => {}
                Err(e) => {
                    let _ = app.emit("reloj-caido", e.to_string());
                }
            }
            std::thread::sleep(REINTENTO);
        }
    });
}

fn suscribirse(app: &AppHandle) -> Result<()> {
    let mut cliente = Client::connect(&paths::socket())?;
    let inicial = cliente.send(&Request::Subscribe)?;
    app.emit("snapshot", &inicial)?;

    while let Some(snap) = cliente.next_snapshot()? {
        app.emit("snapshot", &snap)?;
    }
    Ok(())
}

/// Mira el nombre del tema activo y repinta cuando cambia. Se vigila
/// `theme.name` y no `colors.toml` porque el segundo se reescribe entero al
/// cambiar de tema y se puede leer a medias.
fn seguir_al_tema(app: AppHandle) {
    std::thread::spawn(move || {
        let mut ultimo = std::fs::read_to_string(tema::ruta_nombre()).unwrap_or_default();
        loop {
            std::thread::sleep(OJO_AL_TEMA);
            let ahora = std::fs::read_to_string(tema::ruta_nombre()).unwrap_or_default();
            if ahora != ultimo {
                ultimo = ahora;
                let _ = app.emit("tema", tema::leer());
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let db = Db::open(&paths::database())?;
            app.manage(Estado { db: Mutex::new(db) });
            seguir_al_reloj(app.handle().clone());
            seguir_al_tema(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![estado, mandar, tema, proyectos, tareas])
        .run(tauri::generate_context!())
        .expect("la ventana no pudo arrancar");
}
