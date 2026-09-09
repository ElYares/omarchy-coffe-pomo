//! La ventana de coffe.
//!
//! No tiene reloj propio: todo lo que enseña se lo pregunta al daemon por el
//! mismo socket que usa la CLI. Cerrar la ventana no para un pomodoro, y abrir
//! dos ventanas no crea dos relojes.

mod tema;

use anyhow::Result;
use coffe_core::agenda::{Plan, Vencimiento, planificar_todo};
use coffe_core::config::Config;
use coffe_core::db::projects::{NuevoProyecto, Project};
use coffe_core::db::reportes::{CargaProyecto, Exportacion, Precision, ResumenPeriodo};
use coffe_core::db::tasks::{CambiosTarea, FiltroTareas, NuevaTarea, Task};
use coffe_core::model::{InterruptionKind, Priority, TaskState};
use coffe_core::tablero::{Destino, Escritura, Movida, decidir};
use coffe_core::vault::Cuerpo;
use coffe_core::vault::{leer_backlog, proyectos_con_backlog};
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

/// Una carpeta del vault con Backlog, para poder elegirla de una lista en vez
/// de teclear su nombre.
#[derive(Debug, Clone, Serialize)]
struct CarpetaVault {
    nombre: String,
    /// Historias que son trabajo pendiente. Cero significa que no hay nada que
    /// traerse, y eso conviene verlo ANTES de ligar.
    vivas: u32,
    /// El proyecto que ya la tiene ligada, si hay alguno.
    ligada_a: Option<String>,
}

/// El reporte entero, de un viaje.
///
/// Las tres preguntas se piden juntas porque se leen juntas: un total de
/// pomodoros sin saber en qué proyectos cayeron ni si tus estimaciones valen
/// algo es un número de adorno.
///
/// Las cuentas derivadas —la tasa de anulación y el factor— van YA CALCULADAS
/// por el núcleo. Si la ventana las repitiera con su propia aritmética,
/// tendríamos dos versiones del mismo número esperando a discrepar.
#[derive(Debug, Clone, Serialize)]
struct ReporteVista {
    dias: u32,
    resumen: ResumenPeriodo,
    tasa_anulacion: f64,
    cargas: Vec<CargaProyecto>,
    precision: Precision,
    factor: Option<f64>,
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

// ------------------------------------------------------ los proyectos

#[tauri::command]
fn crear_proyecto(
    estado: State<'_, Estado>,
    nombre: String,
    parent_id: Option<i64>,
) -> Result<i64, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.crear_proyecto(
        NuevoProyecto { parent_id, name: nombre, repo_path: None, vault_path: None },
        chrono::Utc::now(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn renombrar_proyecto(estado: State<'_, Estado>, id: i64, nombre: String) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.renombrar_proyecto(id, &nombre).map_err(|e| e.to_string())
}

/// Lo cuelga de otro padre, o de la raíz con `null`.
///
/// El núcleo se niega a colgarlo de su propio descendiente: eso partiría el
/// árbol en dos y el subárbol no se podría volver a alcanzar.
#[tauri::command]
fn mover_proyecto(
    estado: State<'_, Estado>,
    id: i64,
    parent_id: Option<i64>,
) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.mover_proyecto(id, parent_id).map_err(|e| e.to_string())
}

/// La carpeta del repo. Se guarda **absoluta**: se compara contra el directorio
/// de otra sesión, que puede estar en cualquier sitio.
#[tauri::command]
fn fijar_repo(estado: State<'_, Estado>, id: i64, ruta: Option<String>) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let absoluta = match ruta.filter(|r| !r.trim().is_empty()) {
        Some(r) => Some(
            std::fs::canonicalize(&r)
                .map_err(|_| format!("no existe la carpeta {r}"))?
                .display()
                .to_string(),
        ),
        None => None,
    };
    db.fijar_repo(id, absoluta.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn fijar_vault(estado: State<'_, Estado>, id: i64, carpeta: Option<String>) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.fijar_vault(id, carpeta.filter(|c| !c.trim().is_empty()).as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn archivar_proyecto(estado: State<'_, Estado>, id: i64, archivado: bool) -> Result<u32, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.archivar_proyecto(id, archivado).map_err(|e| e.to_string())
}

/// Cuántos subproyectos y tareas cuelgan de él. Es lo que hay que enseñar antes
/// de dejar borrar: el esquema borra en cascada y con las tareas se va su
/// tiempo medido, que es lo único que esto no puede reconstruir.
#[tauri::command]
fn contenido_proyecto(estado: State<'_, Estado>, id: i64) -> Result<(u32, u32), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.contenido_proyecto(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn borrar_proyecto(estado: State<'_, Estado>, id: i64, force: bool) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.borrar_proyecto(id, force).map_err(|e| e.to_string())?;
    drop(db);
    avisar_al_reloj();
    Ok(())
}

/// Las carpetas del vault que tienen Backlog, con cuántas historias vivas hay.
#[tauri::command]
fn carpetas_vault(estado: State<'_, Estado>) -> Result<Vec<CarpetaVault>, String> {
    let Some(raiz) = estado.cfg.vault_raiz() else { return Ok(Vec::new()) };
    let dir = &estado.cfg.vault.projects_dir;
    let nombres = proyectos_con_backlog(&raiz, dir).map_err(|e| e.to_string())?;

    let db = estado.db.lock().map_err(|e| e.to_string())?;
    nombres
        .into_iter()
        .map(|nombre| {
            let notas = leer_backlog(&raiz, dir, &nombre).map_err(|e| e.to_string())?;
            let ligada = db.proyecto_por_vault(&nombre).map_err(|e| e.to_string())?;
            Ok(CarpetaVault {
                vivas: notas.iter().filter(|n| !n.nota.terminal).count() as u32,
                ligada_a: match ligada {
                    Some(p) => Some(db.ruta_proyecto(p.id).map_err(|e| e.to_string())?),
                    None => None,
                },
                nombre,
            })
        })
        .collect()
}

/// Trae el backlog de un proyecto ya ligado. Devuelve el parte de lo que pasó,
/// incluidas las rarezas: un importador que se traga lo que no entiende pierde
/// trabajo sin que nadie se entere hasta meses después.
#[tauri::command]
fn importar_vault(
    estado: State<'_, Estado>,
    id: i64,
) -> Result<coffe_core::db::tasks::Importacion, String> {
    let Some(raiz) = estado.cfg.vault_raiz() else {
        return Err("no hay vault configurado en config.toml".into());
    };
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let p = db.proyecto(id).map_err(|e| e.to_string())?;
    let Some(carpeta) = p.vault_path else {
        return Err("este proyecto no está ligado a ninguna carpeta del vault".into());
    };

    let notas =
        leer_backlog(&raiz, &estado.cfg.vault.projects_dir, &carpeta).map_err(|e| e.to_string())?;
    let r =
        db.sincronizar_notas(id, &notas, false, chrono::Utc::now()).map_err(|e| e.to_string())?;
    drop(db);
    avisar_al_reloj();
    Ok(r)
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

impl From<Columna> for Destino {
    fn from(c: Columna) -> Self {
        match c {
            Columna::Pending => Destino::Pendiente,
            Columna::InProgress => Destino::EnCurso,
            Columna::Paused => Destino::Refri,
            Columna::Done => Destino::Hecha,
        }
    }
}

/// Lleva una tarea a un destino, sea cual sea quien lo pida.
///
/// La decisión la toma `coffe_core::tablero::decidir`, que es la MISMA que usa
/// la CLI. Aquí solo se junta el estado del mundo —qué dice el reloj, qué dice
/// la base— y se ejecuta lo que salga.
fn llevar(estado: &State<'_, Estado>, id: i64, destino: Destino) -> Result<(), String> {
    let ahora = client::ask(&paths::socket(), &Request::Status).map_err(|e| e.to_string())?;
    let hay_reloj = !ahora.state.is_idle();
    let es_la_activa = hay_reloj && ahora.task.map(|t| t.id) == Some(id);

    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tarea = db.tarea(id).map_err(|e| e.to_string())?;

    match decidir(destino, id, es_la_activa, hay_reloj, tarea.state) {
        Movida::Nada => return Ok(()),
        Movida::Rechazar(por_que) => return Err(por_que.into()),
        Movida::AlReloj(orden) => {
            drop(db);
            client::ask(&paths::socket(), &orden.into()).map_err(|e| e.to_string())?;
            return Ok(());
        }
        Movida::ALaBase(escritura) => {
            let ahora = chrono::Utc::now();
            match escritura {
                Escritura::Aparcar => db.aparcar(id),
                Escritura::Completar => db.completar(id, ahora),
                Escritura::Reabrir => db.reabrir(id),
                Escritura::Archivar => db.archivar_tarea(id, ahora),
            }
            .map_err(|e| e.to_string())?;
        }
    }

    drop(db);
    avisar_al_reloj();
    Ok(())
}

#[tauri::command]
fn mover_a_columna(estado: State<'_, Estado>, id: i64, columna: Columna) -> Result<(), String> {
    llevar(&estado, id, columna.into())
}

/// Apartar una tarea: trabajo que ya no se va a hacer. No es una columna del
/// tablero —el bote no se enseña— pero tenía que existir en algún sitio: hasta
/// ahora la única forma de sacar algo de la vista era borrarlo, y eso pierde el
/// historial.
#[tauri::command]
fn archivar_tarea(estado: State<'_, Estado>, id: i64) -> Result<(), String> {
    llevar(&estado, id, Destino::Archivada)
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
                vault_id: None,
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

/// Pone o quita la estimación.
///
/// Va en su propio comando porque en JSON no hay forma de distinguir "no toques
/// este campo" de "déjalo vacío": las dos cosas llegan como `null`. Aquí el
/// parámetro siempre viene, así que `null` solo puede significar borrar.
#[tauri::command]
fn estimar(estado: State<'_, Estado>, id: i64, pomodoros: Option<u32>) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.editar_tarea(id, &CambiosTarea { estimate_pomodoros: Some(pomodoros), ..Default::default() })
        .map_err(|e| e.to_string())
}

/// Pone o quita la fecha de entrega.
///
/// Mismo trato que `estimar`, y por la misma razon: el parametro siempre viene,
/// asi que `null` solo puede significar borrar. Con `Option<Option<String>>` no
/// se podria —serde convierte `null` en `None`, que aqui significa "no toques
/// nada"— y el boton de quitar la fecha fallaria sin decir nada.
///
/// Sin fecha una tarea no existe para el calendario, asi que esto es lo que
/// decide si entra en la cuenta de "me cabe" o se queda solo en el tablero.
#[tauri::command]
fn fijar_entrega(estado: State<'_, Estado>, id: i64, fecha: Option<String>) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    db.editar_tarea(id, &CambiosTarea { due_date: Some(fecha), ..Default::default() })
        .map_err(|e| e.to_string())
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

/// Abre la nota de una tarea en Obsidian.
///
/// Se usa el esquema `obsidian://` y no `xdg-open` sobre el archivo: abrir el
/// `.md` con el editor por defecto te saca del vault y pierdes los enlaces, que
/// es justo lo que hace útil a la nota.
/// El cuerpo de la nota del vault, para leerla sin salir de la ventana.
///
/// Solo lee. Editar la nota sigue siendo cosa de Obsidian —"Abrir la nota"
/// sigue ahi— porque una nota del vault es la fuente y este es un visor: dos
/// editores sobre el mismo archivo se pisan, y el que pierde es el que no
/// estaba mirando.
#[tauri::command]
fn leer_nota(estado: State<'_, Estado>, id: i64) -> Result<Cuerpo, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tarea = db.tarea(id).map_err(|e| e.to_string())?;
    let Some(nota) = tarea.vault_note else {
        return Err("esta tarea no vino del vault".into());
    };
    // `vault_raiz()` y no `cfg.vault.path` a secas: el path del config lleva `~`
    // y `Path::new("~/...")` es una carpeta llamada "~", que no existe.
    let Some(raiz) = estado.cfg.vault_raiz() else {
        return Err("no hay vault configurado".into());
    };
    coffe_core::vault::leer_nota(&raiz, &nota).map_err(|e| e.to_string())
}

#[tauri::command]
fn abrir_nota(estado: State<'_, Estado>, id: i64) -> Result<(), String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tarea = db.tarea(id).map_err(|e| e.to_string())?;
    let Some(nota) = tarea.vault_note else {
        return Err("esta tarea no vino del vault".into());
    };

    // El nombre del vault es el de su carpeta, que es como lo registra Obsidian.
    let raiz = estado.cfg.vault.path.trim_end_matches('/');
    let vault = raiz.rsplit('/').next().unwrap_or_default();
    if vault.is_empty() {
        return Err("no hay vault configurado".into());
    }
    let archivo = nota.strip_suffix(".md").unwrap_or(&nota);

    let uri = format!("obsidian://open?vault={}&file={}", urlencode(vault), urlencode(archivo));
    std::process::Command::new("xdg-open").arg(&uri).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Lo justo para una URI: los nombres de las notas llevan espacios, acentos y
/// alguna `&`, y cualquiera de los tres parte el enlace.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            otro => out.push_str(&format!("%{otro:02X}")),
        }
    }
    out
}

/// El plan de los próximos días: qué vence, cuánto se debe y si cabe.
///
/// La cuenta la hace `coffe_core::agenda`, que es puro y está probado. Aquí
/// solo se junta lo que hay en la base con la configuración.
#[tauri::command]
fn agenda(estado: State<'_, Estado>, dias: u32) -> Result<Plan, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let tareas = db.tareas(&FiltroTareas::default()).map_err(|e| e.to_string())?;

    // Lo hecho ya no se debe, y lo que no tiene fecha no vence.
    let comprometidas = tareas
        .iter()
        .filter(|t| t.state != TaskState::Done && t.state != TaskState::Archived)
        .filter(|t| t.due_date.is_some());

    let mut vencimientos = Vec::new();
    let mut sin_estimar = 0;

    for t in comprometidas {
        let Some(fecha) = t.due_date.as_deref().and_then(|d| d.parse().ok()) else { continue };
        // Sin estimación no hay número que sumar. Se cuenta aparte en vez de
        // ignorarla: inventarle un valor dejaría el total sin significado, y
        // callarla haría que el plan dijera "todo cabe" sin haber mirado.
        let Some(estimados) = t.estimate_pomodoros else {
            sin_estimar += 1;
            continue;
        };
        let hechos = db.pomodoros_de_tarea(t.id).unwrap_or(0);
        vencimientos.push(Vencimiento { fecha, pomodoros: estimados.saturating_sub(hechos) });
    }

    let hoy = chrono::Local::now().date_naive();
    Ok(planificar_todo(hoy, &vencimientos, sin_estimar, dias.clamp(1, 180), &estado.cfg.agenda))
}

/// En qué se te fue el tiempo. Es lo mismo que enseña `coffe report`, pintado
/// en la ventana en vez de en la terminal.
#[tauri::command]
fn reporte(estado: State<'_, Estado>, dias: u32) -> Result<ReporteVista, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let dias = dias.clamp(1, 3650);
    let hasta = chrono::Utc::now();
    let desde = hasta - chrono::Duration::days(dias as i64);

    let resumen = db.resumen_periodo(desde, hasta).map_err(|e| e.to_string())?;
    let cargas = db.carga_por_proyecto(desde, hasta).map_err(|e| e.to_string())?;
    let precision = db.precision_estimacion().map_err(|e| e.to_string())?;

    Ok(ReporteVista {
        dias,
        tasa_anulacion: resumen.tasa_de_anulacion(),
        factor: precision.factor(),
        resumen,
        cargas,
        precision,
    })
}

/// Los mismos CSV que `coffe export`, escritos donde diga el selector del
/// sistema. El texto lo arma `coffe_core`: la ventana y la terminal tienen que
/// sacar el mismo archivo o no es una exportación, son dos.
///
/// Devuelve cuántas filas fueron. Un archivo de cero filas escrito sin una
/// queja es la clase de éxito que se descubre tarde.
#[tauri::command]
fn exportar_csv(
    estado: State<'_, Estado>,
    que: String,
    dias: u32,
    destino: String,
) -> Result<usize, String> {
    let db = estado.db.lock().map_err(|e| e.to_string())?;
    let que = match que.as_str() {
        "pomodoros" => Exportacion::Pomodoros { dias },
        "tareas" => Exportacion::Tareas,
        otro => return Err(format!("no sé exportar «{otro}»")),
    };
    let (texto, filas) = db.csv(que).map_err(|e| e.to_string())?;
    std::fs::write(&destino, texto).map_err(|e| format!("no se pudo escribir {destino}: {e}"))?;
    Ok(filas)
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
        // Elegir la carpeta de un repo se hace con el selector del sistema. Que
        // una interfaz gráfica te pida teclear una ruta absoluta es justo lo
        // que no debería pasar.
        .plugin(tauri_plugin_dialog::init())
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
            archivar_tarea,
            crear_tarea,
            cambiar_prioridad,
            estimar,
            fijar_entrega,
            editar_tarea,
            mover_de_proyecto,
            borrar_tarea,
            vaciar_papelera,
            agenda,
            reporte,
            exportar_csv,
            config,
            abrir_nota,
            leer_nota,
            crear_proyecto,
            renombrar_proyecto,
            mover_proyecto,
            fijar_repo,
            fijar_vault,
            archivar_proyecto,
            contenido_proyecto,
            borrar_proyecto,
            carpetas_vault,
            importar_vault
        ])
        .run(tauri::generate_context!())
        .expect("la ventana no pudo arrancar");
}
