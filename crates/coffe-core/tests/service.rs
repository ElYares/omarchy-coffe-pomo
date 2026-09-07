//! El puente entre la máquina y la base. Aquí se comprueba que cada efecto
//! acaba escrito donde toca — que es donde viven los errores que no se ven:
//! un pomodoro que se cierra a la hora equivocada no rompe nada, solo miente.

use chrono::{DateTime, Duration, TimeZone, Utc};
use coffe_core::config::{Config, Pomodoro};
use coffe_core::db::Db;
use coffe_core::db::projects::NuevoProyecto;
use coffe_core::db::tasks::NuevaTarea;
use coffe_core::machine::{Command, TimerState};
use coffe_core::model::{Priority, TaskState, VoidReason};
use coffe_core::service::Service;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
}

fn cfg() -> Config {
    Config::default()
}

/// Una base con un proyecto y una tarea, lista para trabajar.
fn base() -> (Db, i64) {
    let db = Db::en_memoria().unwrap();
    let p = db
        .crear_proyecto(
            NuevoProyecto { parent_id: None, name: "strapp".into(), ..Default::default() },
            t0(),
        )
        .unwrap();
    let t = db
        .crear_tarea(
            NuevaTarea {
                project_id: p,
                title: "integración".into(),
                notes: None,
                priority: Priority::High,
                estimate_pomodoros: Some(3),
                due_date: None,
                vault_note: None,
            },
            t0(),
        )
        .unwrap();
    (db, t)
}

fn servicio(db: Db, config: Config, now: DateTime<Utc>) -> Service {
    Service::arrancar(db, config, now).unwrap().0
}

#[test]
fn arrancar_deja_pomodoro_tarea_y_tramo_abiertos() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());

    svc.start(tarea, t0()).unwrap();

    let (_, task_id, inicio) = svc.db().pomodoro_abierto().unwrap().unwrap();
    assert_eq!(task_id, tarea);
    assert_eq!(inicio, t0());
    assert_eq!(svc.db().tarea(tarea).unwrap().state, TaskState::InProgress);
    assert_eq!(svc.db().tarea(tarea).unwrap().first_started_at, Some(t0()));
    // Tramo abierto: aún no suma nada, pero existe.
    assert_eq!(svc.db().resumen_tarea(tarea).unwrap().veces_aparcada, 0);
}

#[test]
fn el_pomodoro_se_cierra_en_el_minuto_25_aunque_el_tick_llegue_tarde() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());
    svc.start(tarea, t0()).unwrap();

    // El daemon despertó un minuto tarde: dentro de la gracia, así que suena.
    svc.ejecutar(Command::Tick, t0() + Duration::minutes(26)).unwrap();

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.pomodoros_completados, 1);
    assert_eq!(
        r.segundos_efectivos, 1500,
        "el pomodoro dura lo que se planeó, no lo que tardó el daemon en enterarse"
    );
    assert!(svc.db().pomodoro_abierto().unwrap().is_none());
}

#[test]
fn pausar_anula_el_pomodoro_y_manda_la_tarea_al_refri() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());
    svc.start(tarea, t0()).unwrap();

    svc.pause(t0() + Duration::minutes(10)).unwrap();

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.pomodoros_completados, 0);
    assert_eq!(r.pomodoros_anulados, 1);
    assert_eq!(r.segundos_efectivos, 0, "un pomodoro tirado no aporta tiempo");
    assert_eq!(r.segundos_en_tramos, 600);
    assert_eq!(r.veces_aparcada, 1);
    assert_eq!(svc.db().tarea(tarea).unwrap().state, TaskState::Paused);
}

#[test]
fn el_ciclo_entero_deja_las_cuentas_cuadradas() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());

    svc.start(tarea, t0()).unwrap();
    svc.ejecutar(Command::Tick, t0() + Duration::minutes(25)).unwrap();
    assert!(matches!(svc.state(), TimerState::Break { .. }));

    svc.ejecutar(Command::Tick, t0() + Duration::minutes(30)).unwrap();
    assert!(svc.state().is_idle());

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.pomodoros_completados, 1);
    assert_eq!(
        r.segundos_en_tramos, 1800,
        "el tramo cubre el pomodoro y su descanso, y se cierra ahí"
    );
    assert_eq!(r.veces_aparcada, 0, "acabar un ciclo no es aparcar nada");
    assert_eq!(svc.db().pomodoros_hoy(t0() + Duration::minutes(30)).unwrap(), 1);
}

#[test]
fn terminar_a_media_pomodoro_cierra_la_tarea_pero_no_el_reloj() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());
    svc.start(tarea, t0()).unwrap();

    svc.done(t0() + Duration::minutes(9)).unwrap();

    assert_eq!(svc.db().tarea(tarea).unwrap().state, TaskState::Done);
    assert!(svc.db().pomodoro_abierto().unwrap().is_some(), "el pomodoro sigue vivo");

    svc.ejecutar(Command::Tick, t0() + Duration::minutes(25)).unwrap();

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.pomodoros_completados, 1, "el repaso también cuenta como pomodoro");
    assert_eq!(r.segundos_en_tramos, 1500);
}

#[test]
fn la_recuperacion_conserva_el_foco_que_seguia_vivo() {
    let dir = std::env::temp_dir().join(format!("coffe-svc-vivo-{}", std::process::id()));
    let ruta = dir.join("coffe.db");
    let _ = std::fs::remove_dir_all(&dir);

    let tarea = {
        let db = Db::open(&ruta).unwrap();
        let p = db
            .crear_proyecto(
                NuevoProyecto { parent_id: None, name: "strapp".into(), ..Default::default() },
                t0(),
            )
            .unwrap();
        let t = db
            .crear_tarea(
                NuevaTarea {
                    project_id: p,
                    title: "x".into(),
                    notes: None,
                    priority: Priority::Medium,
                    estimate_pomodoros: None,
                    due_date: None,
                    vault_note: None,
                },
                t0(),
            )
            .unwrap();
        let mut svc = servicio(db, cfg(), t0());
        svc.start(t, t0()).unwrap();
        t
    };

    // Vuelve a los tres minutos: el pomodoro sigue en pie.
    let db = Db::open(&ruta).unwrap();
    let (svc, fx) = Service::arrancar(db, cfg(), t0() + Duration::minutes(3)).unwrap();

    assert!(fx.is_empty(), "nada que recuperar: no había vencido");
    assert!(matches!(svc.state(), TimerState::Focus { .. }));
    assert!(svc.db().pomodoro_abierto().unwrap().is_some());
    assert_eq!(svc.db().tarea(tarea).unwrap().state, TaskState::InProgress);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn un_pomodoro_que_vencio_con_el_daemon_caido_se_anula() {
    let dir = std::env::temp_dir().join(format!("coffe-svc-perdido-{}", std::process::id()));
    let ruta = dir.join("coffe.db");
    let _ = std::fs::remove_dir_all(&dir);

    let tarea = {
        let db = Db::open(&ruta).unwrap();
        let p = db
            .crear_proyecto(
                NuevoProyecto { parent_id: None, name: "strapp".into(), ..Default::default() },
                t0(),
            )
            .unwrap();
        let t = db
            .crear_tarea(
                NuevaTarea {
                    project_id: p,
                    title: "x".into(),
                    notes: None,
                    priority: Priority::Medium,
                    estimate_pomodoros: None,
                    due_date: None,
                    vault_note: None,
                },
                t0(),
            )
            .unwrap();
        let mut svc = servicio(db, cfg(), t0());
        svc.start(t, t0()).unwrap();
        t
    };

    // El equipo estuvo apagado dos horas. La campana no la oyó nadie.
    let db = Db::open(&ruta).unwrap();
    let (svc, fx) = Service::arrancar(db, cfg(), t0() + Duration::hours(2)).unwrap();

    assert!(!fx.is_empty(), "la recuperación tiene que decir que pasó algo");
    assert!(svc.state().is_idle());

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.pomodoros_completados, 0, "no se regala un pomodoro que nadie oyó");
    assert_eq!(r.pomodoros_anulados, 1);
    assert_eq!(
        r.segundos_en_tramos, 1500,
        "el tramo se cierra donde habría sonado, no dos horas después"
    );
    assert_eq!(r.veces_aparcada, 0, "una caída no es una pausa del usuario");
    assert!(svc.db().pomodoro_abierto().unwrap().is_none());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cambiar_de_tarea_aparca_una_y_arranca_la_otra() {
    let (db, primera) = base();
    let segunda = db
        .crear_tarea(
            NuevaTarea {
                project_id: db.tarea(primera).unwrap().project_id,
                title: "otra".into(),
                notes: None,
                priority: Priority::Low,
                estimate_pomodoros: None,
                due_date: None,
                vault_note: None,
            },
            t0(),
        )
        .unwrap();
    let mut svc = servicio(db, cfg(), t0());

    svc.start(primera, t0()).unwrap();
    svc.ejecutar(Command::Switch { task_id: segunda }, t0() + Duration::minutes(7)).unwrap();

    assert_eq!(svc.db().tarea(primera).unwrap().state, TaskState::Paused);
    assert_eq!(svc.db().tarea(segunda).unwrap().state, TaskState::InProgress);
    assert_eq!(svc.db().resumen_tarea(primera).unwrap().veces_aparcada, 1);
    assert_eq!(svc.db().resumen_tarea(primera).unwrap().pomodoros_anulados, 1);

    let (_, abierto, _) = svc.db().pomodoro_abierto().unwrap().unwrap();
    assert_eq!(abierto, segunda, "solo puede haber uno, y es el nuevo");
}

#[test]
fn en_modo_flexible_el_descanso_se_puede_cortar() {
    let (db, tarea) = base();
    let flexible =
        Config { pomodoro: Pomodoro { strict: false, ..Pomodoro::default() }, ..Config::default() };
    let mut svc = servicio(db, flexible, t0());

    svc.start(tarea, t0()).unwrap();
    svc.ejecutar(Command::Tick, t0() + Duration::minutes(25)).unwrap();
    svc.ejecutar(Command::SkipBreak, t0() + Duration::minutes(26)).unwrap();

    assert!(svc.state().is_idle());
    // Y el pomodoro queda marcado como no canónico.
    let no_estricto: u32 = svc
        .db()
        .conn()
        .query_row("SELECT COUNT(*) FROM pomodoros WHERE strict = 0", [], |f| f.get(0))
        .unwrap();
    assert_eq!(no_estricto, 1);
}

#[test]
fn una_interrupcion_queda_colgada_de_su_pomodoro() {
    let (db, tarea) = base();
    let mut svc = servicio(db, cfg(), t0());
    svc.start(tarea, t0()).unwrap();

    svc.interrupt(coffe_core::model::InterruptionKind::External, t0() + Duration::minutes(4))
        .unwrap();

    let r = svc.db().resumen_tarea(tarea).unwrap();
    assert_eq!(r.interrupciones_externas, 1);
    let con_pomodoro: u32 = svc
        .db()
        .conn()
        .query_row("SELECT COUNT(*) FROM interruptions WHERE pomodoro_id IS NOT NULL", [], |f| {
            f.get(0)
        })
        .unwrap();
    assert_eq!(con_pomodoro, 1, "se apunta contra el pomodoro que se estaba corriendo");
    // Y el pomodoro sigue vivo.
    assert!(svc.db().pomodoro_abierto().unwrap().is_some());
    let _ = VoidReason::Abandoned;
}
