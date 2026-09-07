//! La ventana de coffe.
//!
//! No tiene reloj propio: todo lo que enseña se lo pregunta al daemon por el
//! mismo socket que usa la CLI. Cerrar la ventana no para un pomodoro, y abrir
//! dos ventanas no crea dos relojes.

mod tema;

use anyhow::Result;
use coffe_core::agenda::{DiaAgenda, Vencimiento, planificar};
use coffe_core::config::Config;
use coffe_core::db::projects::Project;
use coffe_core::db::tasks::{CambiosTarea, FiltroTareas, NuevaTarea, Task};
use coffe_core::model::{InterruptionKind, Priority, TaskState};
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
    cfg: Config,
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

// ------------------------------------------------------- el tablero

/// Las columnas del tablero. Son los estados de una tarea, pero se nombran
/// aparte porque lo que la interfaz puede pedir no es todo lo que una tarea
/// puede ser: `archived` no es una columna, es el fondo del bote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Columna {
    Pending,
    InProgress,
    Paused,
    Done,
}

/// Lo que hay que hacer para llevar una tarjeta a una columna.
///
/// Está separado del comando a propósito: es **la** regla del tablero, y así
/// se puede comprobar entera sin ventana, sin daemon y sin base.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Movida {
    /// No hay nada que mover.
    Nada,
    /// Se lo pedimos al reloj: arrancar, cambiar, aparcar o dar por hecha.
    AlReloj(Request),
    /// La tarea no está bajo el reloj; se escribe directo.
    ALaBase(Escritura),
    /// La columna no admite esa tarjeta, y hay que decir por qué.
    Rechazar(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escritura {
    Aparcar,
    Completar,
    Reabrir,
}

/// **El estado de una tarea lo manda el reloj, no el tablero.** Si la interfaz
/// escribiera el estado por su cuenta, soltar una tarjeta en "en curso" dejaría
/// una tarea marcada como trabajándose sin ningún pomodoro detrás, y las dos
/// mitades de la aplicación contarían cosas distintas del mismo día.
///
/// Así que todo lo que toca al reloj se le pide al daemon, y solo lo que no lo
/// toca va directo a la base.
fn decidir(
    columna: Columna,
    id: i64,
    es_la_activa: bool,
    hay_reloj_corriendo: bool,
    estado: TaskState,
) -> Movida {
    match columna {
        Columna::InProgress if es_la_activa => Movida::Nada,
        // Con un pomodoro vivo, empezar otra cosa es un cambio en caliente:
        // anula el actual y lo deja apuntado como tal. No es lo mismo que
        // arrancar en frío y el daemon tiene que saberlo.
        Columna::InProgress if hay_reloj_corriendo => {
            Movida::AlReloj(Request::Switch { task_id: id })
        }
        Columna::InProgress => Movida::AlReloj(Request::Start { task_id: id }),

        Columna::Paused if es_la_activa => Movida::AlReloj(Request::Pause),
        Columna::Paused if estado == TaskState::InProgress => Movida::ALaBase(Escritura::Aparcar),
        Columna::Paused => Movida::Rechazar("al refri solo va lo que está en curso"),

        // En estricto esto NO calla el pomodoro: la tarea queda hecha y el
        // reloj sigue hasta sonar. Es la regla, no un descuido.
        Columna::Done if es_la_activa => Movida::AlReloj(Request::Done),
        Columna::Done if estado == TaskState::Done => Movida::Nada,
        Columna::Done => Movida::ALaBase(Escritura::Completar),

        Columna::Pending if es_la_activa => {
            Movida::Rechazar("está corriendo: apárcala o termínala antes")
        }
        Columna::Pending if estado == TaskState::Pending => Movida::Nada,
        Columna::Pending => Movida::ALaBase(Escritura::Reabrir),
    }
}

#[tauri::command]
fn mover_a_columna(estado: State<'_, Estado>, id: i64, columna: Columna) -> Result<(), String> {
    let ahora = client::ask(&paths::socket(), &Request::Status).map_err(|e| e.to_string())?;
    let hay_reloj = !ahora.state.is_idle();
    let es_la_activa = hay_reloj && ahora.task.map(|t| t.id) == Some(id);

    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tarea = db.tarea(id).map_err(|e| e.to_string())?;

    match decidir(columna, id, es_la_activa, hay_reloj, tarea.state) {
        Movida::Nada => return Ok(()),
        Movida::Rechazar(por_que) => return Err(por_que.into()),
        Movida::AlReloj(peticion) => {
            drop(db);
            client::ask(&paths::socket(), &peticion).map_err(|e| e.to_string())?;
            return Ok(());
        }
        Movida::ALaBase(escritura) => {
            match escritura {
                Escritura::Aparcar => db.aparcar(id),
                Escritura::Completar => db.completar(id, chrono::Utc::now()),
                Escritura::Reabrir => db.reabrir(id),
            }
            .map_err(|e| e.to_string())?;
        }
    }

    drop(db);
    avisar_al_reloj();
    Ok(())
}

#[tauri::command]
fn crear_tarea(
    estado: State<'_, Estado>,
    project_id: i64,
    title: String,
    priority: Priority,
    due_date: Option<String>,
    estimate_pomodoros: Option<u32>,
) -> Result<i64, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let id = db
        .crear_tarea(
            NuevaTarea {
                project_id,
                title,
                notes: None,
                priority,
                estimate_pomodoros,
                due_date,
                vault_note: None,
            },
            chrono::Utc::now(),
        )
        .map_err(|e| e.to_string())?;
    drop(db);
    avisar_al_reloj();
    Ok(id)
}

#[tauri::command]
fn cambiar_prioridad(estado: State<'_, Estado>, id: i64, priority: Priority) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.cambiar_prioridad(id, priority).map_err(|e| e.to_string())
}

#[tauri::command]
fn editar_tarea(
    estado: State<'_, Estado>,
    id: i64,
    title: Option<String>,
    due_date: Option<Option<String>>,
    estimate_pomodoros: Option<Option<u32>>,
) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.editar_tarea(id, &CambiosTarea { title, due_date, estimate_pomodoros, ..Default::default() })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn mover_de_proyecto(estado: State<'_, Estado>, id: i64, project_id: i64) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.mover_tarea(id, project_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn borrar_tarea(estado: State<'_, Estado>, id: i64, force: bool) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.borrar_tarea(id, force).map_err(|e| e.to_string())?;
    drop(db);
    avisar_al_reloj();
    Ok(())
}

/// Vaciar el bote. No borra: archiva. Los tiempos siguen contando para los
/// reportes aunque la tarjeta desaparezca del tablero.
#[tauri::command]
fn vaciar_papelera(estado: State<'_, Estado>) -> Result<usize, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let n = db.vaciar_papelera(chrono::Utc::now()).map_err(|e| e.to_string())?;
    drop(db);
    avisar_al_reloj();
    Ok(n)
}

/// El plan de los próximos días: qué vence, cuánto se debe y si cabe.
///
/// La cuenta la hace `coffe_core::agenda`, que es puro y está probado. Aquí
/// solo se junta lo que hay en la base con la configuración.
#[tauri::command]
fn agenda(estado: State<'_, Estado>, dias: u32) -> Result<Vec<DiaAgenda>, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tareas = db.tareas(&FiltroTareas::default()).map_err(|e| e.to_string())?;

    let vencimientos: Vec<Vencimiento> = tareas
        .iter()
        // Lo hecho ya no se debe, y lo que no tiene fecha no vence.
        .filter(|t| t.state != TaskState::Done && t.state != TaskState::Archived)
        .filter_map(|t| {
            let fecha = t.due_date.as_deref()?.parse().ok()?;
            // Sin estimación no se puede sumar. Aparece en el calendario como
            // tarea, pero no pesa: inventarle un número sería peor que no
            // contarla, porque el total dejaría de significar nada.
            let estimados = t.estimate_pomodoros?;
            let hechos = db.pomodoros_de_tarea(t.id).unwrap_or(0);
            Some(Vencimiento { fecha, pomodoros: estimados.saturating_sub(hechos) })
        })
        .collect();

    let hoy = chrono::Local::now().date_naive();
    Ok(planificar(hoy, &vencimientos, dias.clamp(1, 180), &estado.cfg.agenda))
}

/// La configuración en uso, para que la ventana no tenga que adivinar cuántos
/// pomodoros caben en un día ni si el modo estricto está puesto.
#[tauri::command]
fn config(estado: State<'_, Estado>) -> Config {
    estado.cfg.clone()
}

/// Un cambio escrito directo en la base no genera ningún evento del reloj, así
/// que la barra seguiría enseñando el número de tazas de antes. Un `Status` lo
/// obliga a releer y a empujar el estado nuevo a todo el mundo.
fn avisar_al_reloj() {
    let _ = client::ask(&paths::socket(), &Request::Status);
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
            let cfg = Config::load(&paths::config())?;
            app.manage(Estado { db: Mutex::new(db), cfg });
            seguir_al_reloj(app.handle().clone());
            seguir_al_tema(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            estado,
            mandar,
            tema,
            proyectos,
            tareas,
            mover_a_columna,
            crear_tarea,
            cambiar_prioridad,
            editar_tarea,
            mover_de_proyecto,
            borrar_tarea,
            vaciar_papelera,
            agenda,
            config
        ])
        .run(tauri::generate_context!())
        .expect("la ventana no pudo arrancar");
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Arrancar es siempre cosa del reloj: el tablero no puede marcar una tarea
    /// como en curso sin que haya un pomodoro detrás.
    #[test]
    fn arrastrar_a_en_curso_siempre_pasa_por_el_reloj() {
        assert_eq!(
            decidir(Columna::InProgress, 7, false, false, TaskState::Pending),
            Movida::AlReloj(Request::Start { task_id: 7 })
        );
        assert_eq!(
            decidir(Columna::InProgress, 7, false, false, TaskState::Paused),
            Movida::AlReloj(Request::Start { task_id: 7 }),
            "sacar del refri tambien es arrancar"
        );
    }

    #[test]
    fn con_un_pomodoro_vivo_empezar_otra_cosa_es_un_cambio_en_caliente() {
        // Un `start` normal dejaria el pomodoro anterior abierto en el aire.
        assert_eq!(
            decidir(Columna::InProgress, 9, false, true, TaskState::Pending),
            Movida::AlReloj(Request::Switch { task_id: 9 })
        );
    }

    #[test]
    fn soltar_la_tarjeta_donde_ya_estaba_no_hace_nada() {
        assert_eq!(
            decidir(Columna::InProgress, 1, true, true, TaskState::InProgress),
            Movida::Nada
        );
        assert_eq!(decidir(Columna::Done, 1, false, false, TaskState::Done), Movida::Nada);
        assert_eq!(decidir(Columna::Pending, 1, false, false, TaskState::Pending), Movida::Nada);
    }

    #[test]
    fn aparcar_la_que_esta_corriendo_pasa_por_el_reloj() {
        assert_eq!(
            decidir(Columna::Paused, 1, true, true, TaskState::InProgress),
            Movida::AlReloj(Request::Pause),
            "hay un pomodoro que anular, y eso no lo puede hacer la base"
        );
    }

    #[test]
    fn al_refri_no_va_lo_que_nunca_empezo() {
        assert!(matches!(
            decidir(Columna::Paused, 1, false, false, TaskState::Pending),
            Movida::Rechazar(_)
        ));
        assert!(matches!(
            decidir(Columna::Paused, 1, false, false, TaskState::Done),
            Movida::Rechazar(_)
        ));
    }

    #[test]
    fn terminar_la_activa_pasa_por_el_reloj_pero_otra_no() {
        // En estricto, `Done` sobre la activa NO calla el pomodoro: lo deja
        // sonar. Por eso tiene que ir al daemon y no a la base.
        assert_eq!(
            decidir(Columna::Done, 3, true, true, TaskState::InProgress),
            Movida::AlReloj(Request::Done)
        );
        assert_eq!(
            decidir(Columna::Done, 4, false, true, TaskState::Pending),
            Movida::ALaBase(Escritura::Completar),
            "una tarea que no esta bajo el reloj se cierra directo"
        );
    }

    #[test]
    fn no_se_devuelve_a_pendiente_algo_que_esta_corriendo() {
        assert!(matches!(
            decidir(Columna::Pending, 1, true, true, TaskState::InProgress),
            Movida::Rechazar(_)
        ));
        assert_eq!(
            decidir(Columna::Pending, 1, false, false, TaskState::Done),
            Movida::ALaBase(Escritura::Reabrir)
        );
    }
}
