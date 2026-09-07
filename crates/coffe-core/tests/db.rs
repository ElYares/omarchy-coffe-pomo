//! La persistencia: el árbol, las reglas que viven en el esquema y las
//! métricas de tiempo.

use chrono::{DateTime, Duration, TimeZone, Utc};
use coffe_core::db::Db;
use coffe_core::db::projects::{NuevoProyecto, slugify};
use coffe_core::db::tasks::{AvisoEstimacion, FiltroTareas, NuevaTarea, revisar_estimacion};
use coffe_core::error::CoffeError;
use coffe_core::machine::{SessionEnd, TimerState};
use coffe_core::model::{InterruptionKind, Priority, TaskState, VoidReason};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
}

fn proyecto(db: &Db, padre: Option<i64>, nombre: &str) -> i64 {
    db.crear_proyecto(
        NuevoProyecto { parent_id: padre, name: nombre.into(), ..Default::default() },
        t0(),
    )
    .unwrap()
}

fn tarea(db: &Db, proyecto_id: i64, titulo: &str) -> i64 {
    db.crear_tarea(
        NuevaTarea {
            project_id: proyecto_id,
            title: titulo.into(),
            notes: None,
            priority: Priority::Medium,
            estimate_pomodoros: Some(2),
            due_date: None,
            vault_note: None,
        },
        t0(),
    )
    .unwrap()
}

#[test]
fn la_base_se_migra_y_volver_a_abrirla_no_rompe_nada() {
    let dir = std::env::temp_dir().join(format!("coffe-test-{}", std::process::id()));
    let ruta = dir.join("coffe.db");
    let _ = std::fs::remove_dir_all(&dir);

    let db = Db::open(&ruta).unwrap();
    let p = proyecto(&db, None, "strapp");
    drop(db);

    let db = Db::open(&ruta).unwrap();
    assert_eq!(db.proyecto(p).unwrap().name, "strapp");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn el_arbol_admite_la_jerarquia_completa() {
    let db = Db::en_memoria().unwrap();
    let personal = proyecto(&db, None, "personal");
    let labs = proyecto(&db, Some(personal), "labs");
    let video = proyecto(&db, Some(labs), "video procesos");

    assert_eq!(db.ruta_proyecto(video).unwrap(), "personal / labs / video procesos");
    assert_eq!(db.proyectos(false).unwrap().len(), 3);
}

#[test]
fn los_proyectos_salen_con_el_padre_antes_que_los_hijos() {
    let db = Db::en_memoria().unwrap();
    let clientes = proyecto(&db, None, "clientes");
    proyecto(&db, Some(clientes), "nutricore");
    let strapp = proyecto(&db, None, "strapp");
    proyecto(&db, Some(strapp), "aang");

    let nombres: Vec<_> = db.proyectos(false).unwrap().into_iter().map(|p| p.name).collect();

    assert_eq!(nombres, ["clientes", "nutricore", "strapp", "aang"]);
}

#[test]
fn dos_hermanos_no_pueden_llamarse_igual_pero_dos_primos_si() {
    let db = Db::en_memoria().unwrap();
    let a = proyecto(&db, None, "strapp");
    let b = proyecto(&db, None, "clientes");

    proyecto(&db, Some(a), "api");
    proyecto(&db, Some(b), "api"); // otro padre, sin problema

    let repetido = db.crear_proyecto(
        NuevoProyecto { parent_id: Some(a), name: "API".into(), ..Default::default() },
        t0(),
    );
    assert!(repetido.is_err(), "el mismo slug bajo el mismo padre no debe entrar");
}

#[test]
fn dos_raices_tampoco_pueden_llamarse_igual() {
    // El COALESCE del índice existe justo por esto: con un UNIQUE normal,
    // SQLite deja pasar dos NULL como si fueran padres distintos.
    let db = Db::en_memoria().unwrap();
    proyecto(&db, None, "strapp");
    let repetido = db.crear_proyecto(
        NuevoProyecto { parent_id: None, name: "strapp".into(), ..Default::default() },
        t0(),
    );
    assert!(repetido.is_err());
}

#[test]
fn el_proyecto_por_cwd_es_el_mas_especifico() {
    let db = Db::en_memoria().unwrap();
    db.crear_proyecto(
        NuevoProyecto {
            parent_id: None,
            name: "personal".into(),
            repo_path: Some("/home/e/develop/personal".into()),
            vault_path: None,
        },
        t0(),
    )
    .unwrap();
    let hijo = db
        .crear_proyecto(
            NuevoProyecto {
                parent_id: None,
                name: "devherd".into(),
                repo_path: Some("/home/e/develop/personal/devherd".into()),
                vault_path: None,
            },
            t0(),
        )
        .unwrap();

    let hallado = db.proyecto_por_ruta("/home/e/develop/personal/devherd/cmd").unwrap();
    assert_eq!(hallado.unwrap().id, hijo, "gana el repo más profundo, no el padre");

    assert!(db.proyecto_por_ruta("/tmp/otra-cosa").unwrap().is_none());
    // Un prefijo de texto no es un prefijo de ruta.
    assert!(db.proyecto_por_ruta("/home/e/develop/personal-viejo").unwrap().is_none());
}

#[test]
fn la_prioridad_se_congela_con_el_primer_pomodoro() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "integración de aang");

    db.cambiar_prioridad(t, Priority::High).unwrap();
    assert_eq!(db.tarea(t).unwrap().priority, Priority::High);

    db.marcar_en_curso(t, t0()).unwrap();

    let err = db.cambiar_prioridad(t, Priority::Low).unwrap_err();
    assert!(
        matches!(err, CoffeError::PrioridadCongelada { estado: "in_progress", .. }),
        "salió {err:?}"
    );
    assert_eq!(db.tarea(t).unwrap().priority, Priority::High, "y no se movió");
}

#[test]
fn el_arranque_de_calendario_se_sella_una_sola_vez() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "tl mas");

    db.marcar_en_curso(t, t0()).unwrap();
    db.aparcar(t).unwrap();
    db.marcar_en_curso(t, t0() + Duration::days(2)).unwrap();

    assert_eq!(
        db.tarea(t).unwrap().first_started_at,
        Some(t0()),
        "volver del refri no reinicia el reloj de calendario"
    );
}

#[test]
fn las_tareas_salen_por_entrega_y_luego_por_prioridad() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "clientes");

    let nueva = |titulo: &str, prio, due: Option<&str>| NuevaTarea {
        project_id: p,
        title: titulo.into(),
        notes: None,
        priority: prio,
        estimate_pomodoros: None,
        due_date: due.map(String::from),
        vault_note: None,
    };

    db.crear_tarea(nueva("sin fecha", Priority::High, None), t0()).unwrap();
    db.crear_tarea(nueva("viernes baja", Priority::Low, Some("2026-09-11")), t0()).unwrap();
    db.crear_tarea(nueva("viernes alta", Priority::High, Some("2026-09-11")), t0()).unwrap();
    db.crear_tarea(nueva("miércoles", Priority::Low, Some("2026-09-09")), t0()).unwrap();

    let titulos: Vec<_> =
        db.tareas(&FiltroTareas::default()).unwrap().into_iter().map(|t| t.title).collect();

    assert_eq!(titulos, ["miércoles", "viernes alta", "viernes baja", "sin fecha"]);
}

#[test]
fn el_filtro_por_proyecto_puede_bajar_por_el_arbol() {
    let db = Db::en_memoria().unwrap();
    let personal = proyecto(&db, None, "personal");
    let labs = proyecto(&db, Some(personal), "labs");
    let video = proyecto(&db, Some(labs), "video");
    tarea(&db, personal, "raíz");
    tarea(&db, video, "nieta");

    let solo = FiltroTareas { project_id: Some(personal), ..Default::default() };
    assert_eq!(db.tareas(&solo).unwrap().len(), 1);

    let con_hijos = FiltroTareas {
        project_id: Some(personal),
        incluir_descendientes: true,
        ..Default::default()
    };
    assert_eq!(db.tareas(&con_hijos).unwrap().len(), 2);
}

#[test]
fn vaciar_la_papelera_archiva_sin_borrar() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let hecha = tarea(&db, p, "hecha");
    let viva = tarea(&db, p, "viva");
    db.completar(hecha, t0()).unwrap();

    assert_eq!(db.vaciar_papelera(t0()).unwrap(), 1);

    assert_eq!(db.tarea(hecha).unwrap().state, TaskState::Archived);
    assert_eq!(db.tarea(viva).unwrap().state, TaskState::Pending);
    // Archivada deja de estorbar en el tablero, pero sigue ahí.
    assert_eq!(db.tareas(&FiltroTareas::default()).unwrap().len(), 1);
}

#[test]
fn no_puede_haber_dos_pomodoros_abiertos() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let a = tarea(&db, p, "a");
    let b = tarea(&db, p, "b");

    db.abrir_pomodoro(a, t0(), 1500, true).unwrap();
    assert!(
        db.abrir_pomodoro(b, t0(), 1500, true).is_err(),
        "el índice parcial tiene que impedirlo aunque el daemon se equivoque"
    );

    db.anular_pomodoro(t0(), VoidReason::Switched).unwrap();
    db.abrir_pomodoro(b, t0(), 1500, true).unwrap();
}

#[test]
fn abrir_un_tramo_dos_veces_no_duplica_el_tiempo() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "a");

    db.abrir_tramo(t, t0()).unwrap();
    db.abrir_tramo(t, t0() + Duration::minutes(5)).unwrap();
    db.cerrar_tramo(t, t0() + Duration::minutes(10), SessionEnd::Idle).unwrap();

    let r = db.resumen_tarea(t).unwrap();
    assert_eq!(r.segundos_en_tramos, 600, "un solo tramo de diez minutos");
    assert_eq!(r.veces_aparcada, 0);
}

#[test]
fn el_resumen_separa_el_tiempo_efectivo_del_de_calendario() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "clientes");
    let t = tarea(&db, p, "integración de nutricore");

    // Lunes: un pomodoro entero y uno tirado por una llamada.
    db.marcar_en_curso(t, t0()).unwrap();
    db.abrir_tramo(t, t0()).unwrap();
    db.abrir_pomodoro(t, t0(), 1500, true).unwrap();
    db.completar_pomodoro(t0() + Duration::minutes(25)).unwrap();

    let pid = db.abrir_pomodoro(t, t0() + Duration::minutes(30), 1500, true).unwrap();
    db.registrar_interrupcion(
        Some(t),
        Some(pid),
        InterruptionKind::External,
        t0() + Duration::minutes(35),
        Some("llamada"),
    )
    .unwrap();
    db.anular_pomodoro(t0() + Duration::minutes(36), VoidReason::Interrupted).unwrap();
    db.cerrar_tramo(t, t0() + Duration::minutes(40), SessionEnd::Parked).unwrap();
    db.aparcar(t).unwrap();

    // Miércoles: se retoma y se cierra.
    let miercoles = t0() + Duration::days(2);
    db.marcar_en_curso(t, miercoles).unwrap();
    db.abrir_tramo(t, miercoles).unwrap();
    db.abrir_pomodoro(t, miercoles, 1500, true).unwrap();
    db.completar_pomodoro(miercoles + Duration::minutes(25)).unwrap();
    db.registrar_interrupcion(
        Some(t),
        None,
        InterruptionKind::Internal,
        miercoles + Duration::minutes(10),
        None,
    )
    .unwrap();
    db.cerrar_tramo(t, miercoles + Duration::minutes(30), SessionEnd::Done).unwrap();
    db.completar(t, miercoles + Duration::minutes(30)).unwrap();

    let r = db.resumen_tarea(t).unwrap();

    assert_eq!(r.pomodoros_completados, 2);
    assert_eq!(r.pomodoros_anulados, 1);
    assert_eq!(r.segundos_efectivos, 3000, "50 minutos que sí valen");
    assert_eq!(r.segundos_en_tramos, 40 * 60 + 30 * 60, "70 minutos sentado");
    assert_eq!(
        r.segundos_calendario,
        Some(2 * 86400 + 30 * 60),
        "dos días y medio de calendario para 50 minutos de trabajo"
    );
    assert_eq!(r.veces_aparcada, 1);
    assert_eq!(r.interrupciones_externas, 1);
    assert_eq!(r.interrupciones_internas, 1);
}

#[test]
fn el_reloj_sobrevive_al_reinicio_del_daemon() {
    let db = Db::en_memoria().unwrap();
    assert!(db.leer_timer().unwrap().is_none());

    let estado = TimerState::Focus {
        task_id: 3,
        started_at: t0(),
        ends_at: t0() + Duration::minutes(25),
        overlearning: false,
    };
    db.guardar_timer(&estado, 2, t0()).unwrap();
    db.guardar_timer(&estado, 3, t0()).unwrap(); // una sola fila, siempre

    let (leido, n) = db.leer_timer().unwrap().unwrap();
    assert_eq!(leido, estado);
    assert_eq!(n, 3);
}

#[test]
fn los_tramos_huerfanos_se_cierran_al_arrancar() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "a");
    db.abrir_tramo(t, t0()).unwrap();

    assert_eq!(db.cerrar_tramos_huerfanos(t0() + Duration::hours(1)).unwrap(), 1);
    assert_eq!(db.resumen_tarea(t).unwrap().segundos_en_tramos, 3600);
}

#[test]
fn la_estimacion_avisa_de_lo_que_cirillo_no_deja_pasar() {
    assert_eq!(revisar_estimacion(Some(3), 7), None);
    assert_eq!(revisar_estimacion(None, 7), None);
    assert_eq!(revisar_estimacion(Some(0), 7), Some(AvisoEstimacion::DemasiadoChica));
    assert_eq!(
        revisar_estimacion(Some(9), 7),
        Some(AvisoEstimacion::DemasiadoGrande { estimados: 9, max: 7 })
    );
}

#[test]
fn el_slug_aguanta_acentos_y_espacios() {
    assert_eq!(slugify("Luz Gutiérrez"), "luz-gutierrez");
    assert_eq!(slugify("  TL más  "), "tl-mas");
    assert_eq!(slugify("video/procesos"), "video-procesos");
    assert_eq!(slugify("Ñandú"), "nandu");
    assert_eq!(slugify("---"), "");
}

#[test]
fn solo_las_pausas_deliberadas_cuentan_como_pausa() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "a");

    // Tres ciclos que acabaron solos y uno que se aparcó a propósito.
    for i in 0..3 {
        let ini = t0() + Duration::minutes(i * 60);
        db.abrir_tramo(t, ini).unwrap();
        db.cerrar_tramo(t, ini + Duration::minutes(30), SessionEnd::Idle).unwrap();
    }
    let ini = t0() + Duration::minutes(300);
    db.abrir_tramo(t, ini).unwrap();
    db.cerrar_tramo(t, ini + Duration::minutes(10), SessionEnd::Parked).unwrap();

    let r = db.resumen_tarea(t).unwrap();
    assert_eq!(r.veces_aparcada, 1, "tres ciclos normales no son tres pausas");
    assert_eq!(r.segundos_en_tramos, 100 * 60);
}
