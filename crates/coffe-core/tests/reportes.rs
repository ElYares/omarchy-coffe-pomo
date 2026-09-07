//! Los reportes. Si estos mienten, todo lo demás fue tiempo perdido: son la
//! única razón por la que se mide.

use chrono::{DateTime, Duration, TimeZone, Utc};
use coffe_core::db::Db;
use coffe_core::db::projects::NuevoProyecto;
use coffe_core::db::tasks::NuevaTarea;
use coffe_core::model::{InterruptionKind, Priority, VoidReason};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
}

fn semana() -> (DateTime<Utc>, DateTime<Utc>) {
    (t0() - Duration::days(1), t0() + Duration::days(7))
}

fn proyecto(db: &Db, padre: Option<i64>, nombre: &str) -> i64 {
    db.crear_proyecto(
        NuevoProyecto { parent_id: padre, name: nombre.into(), ..Default::default() },
        t0(),
    )
    .unwrap()
}

fn tarea(db: &Db, p: i64, titulo: &str, estimate: Option<u32>) -> i64 {
    db.crear_tarea(
        NuevaTarea {
            project_id: p,
            title: titulo.into(),
            notes: None,
            priority: Priority::Medium,
            estimate_pomodoros: estimate,
            due_date: None,
            vault_note: None,
            vault_id: None,
        },
        t0(),
    )
    .unwrap()
}

/// Un pomodoro que sonó.
fn suena(db: &Db, t: i64, desde: DateTime<Utc>) {
    db.abrir_pomodoro(t, desde, 1500, true).unwrap();
    db.completar_pomodoro(desde + Duration::minutes(25)).unwrap();
}

/// Uno que se tiró.
fn se_tira(db: &Db, t: i64, desde: DateTime<Utc>) {
    db.abrir_pomodoro(t, desde, 1500, true).unwrap();
    db.anular_pomodoro(desde + Duration::minutes(9), VoidReason::Interrupted).unwrap();
}

#[test]
fn el_resumen_separa_lo_que_sono_de_lo_que_se_tiro() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "una", None);

    suena(&db, t, t0());
    suena(&db, t, t0() + Duration::hours(1));
    se_tira(&db, t, t0() + Duration::hours(2));

    let (d, h) = semana();
    let r = db.resumen_periodo(d, h).unwrap();

    assert_eq!(r.pomodoros_completados, 2);
    assert_eq!(r.pomodoros_anulados, 1);
    assert_eq!(r.segundos_efectivos, 3000, "solo cuenta lo que sonó");
    assert!((r.tasa_de_anulacion() - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn los_dias_con_trabajo_le_dan_sentido_al_total() {
    // Cuarenta pomodoros en cuatro días no es lo mismo que en veinte.
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "una", None);

    for dia in 0..3 {
        suena(&db, t, t0() + Duration::days(dia));
        suena(&db, t, t0() + Duration::days(dia) + Duration::hours(2));
    }

    let (d, h) = semana();
    let r = db.resumen_periodo(d, h).unwrap();

    assert_eq!(r.pomodoros_completados, 6);
    assert_eq!(r.dias_con_trabajo, 3);
}

#[test]
fn lo_de_fuera_del_periodo_no_entra() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "una", None);

    suena(&db, t, t0() - Duration::days(30));
    suena(&db, t, t0());

    let r = db.resumen_periodo(t0() - Duration::days(1), t0() + Duration::days(1)).unwrap();
    assert_eq!(r.pomodoros_completados, 1);
}

#[test]
fn un_proyecto_padre_suma_lo_de_sus_hijos() {
    // Preguntar cuánto se lleva `strapp` y que no cuente lo de `strapp/tl-mas`
    // sería una jerarquía de adorno.
    let db = Db::en_memoria().unwrap();
    let strapp = proyecto(&db, None, "strapp");
    let tlmas = proyecto(&db, Some(strapp), "tl-mas");
    let otro = proyecto(&db, None, "clientes");

    let a = tarea(&db, strapp, "del padre", None);
    let b = tarea(&db, tlmas, "del hijo", None);
    let c = tarea(&db, otro, "de otro", None);

    suena(&db, a, t0());
    suena(&db, b, t0() + Duration::hours(1));
    suena(&db, b, t0() + Duration::hours(2));
    suena(&db, c, t0() + Duration::hours(3));

    let (d, h) = semana();
    let cargas = db.carga_por_proyecto(d, h).unwrap();
    let de = |ruta: &str| cargas.iter().find(|c| c.ruta == ruta).unwrap().pomodoros;

    assert_eq!(de("strapp"), 3, "uno suyo y dos del hijo");
    assert_eq!(de("strapp / tl-mas"), 2);
    assert_eq!(de("clientes"), 1);
}

#[test]
fn la_carga_sale_del_que_mas_se_lleva_al_que_menos() {
    let db = Db::en_memoria().unwrap();
    let poco = proyecto(&db, None, "poco");
    let mucho = proyecto(&db, None, "mucho");
    let a = tarea(&db, poco, "a", None);
    let b = tarea(&db, mucho, "b", None);

    suena(&db, a, t0());
    for i in 0..3 {
        suena(&db, b, t0() + Duration::hours(i + 1));
    }

    let (d, h) = semana();
    let cargas = db.carga_por_proyecto(d, h).unwrap();

    assert_eq!(cargas[0].ruta, "mucho");
    assert_eq!(cargas.len(), 2, "un proyecto sin nada no ocupa una línea");
}

#[test]
fn la_precision_compara_lo_dicho_con_lo_gastado() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");

    // Dijo 2, gastó 4: se quedó corto.
    let corta = tarea(&db, p, "corta", Some(2));
    for i in 0..4 {
        suena(&db, corta, t0() + Duration::hours(i));
    }
    db.completar(corta, t0() + Duration::hours(5)).unwrap();

    // Dijo 3, gastó 3.
    let clavada = tarea(&db, p, "clavada", Some(3));
    for i in 0..3 {
        suena(&db, clavada, t0() + Duration::days(1) + Duration::hours(i));
    }
    db.completar(clavada, t0() + Duration::days(1) + Duration::hours(4)).unwrap();

    let r = db.precision_estimacion().unwrap();

    assert_eq!(r.tareas, 2);
    assert_eq!((r.estimados, r.reales), (5, 7));
    assert_eq!((r.subestimadas, r.clavadas, r.sobreestimadas), (1, 1, 0));
    assert!((r.factor().unwrap() - 1.4).abs() < 1e-9, "cuando dices 5, gastas 7");
}

#[test]
fn una_tarea_a_medias_no_entra_en_la_precision() {
    // Todavía puede crecer: contarla ahora diría que sobreestimas.
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "a medias", Some(5));
    suena(&db, t, t0());

    assert_eq!(db.precision_estimacion().unwrap().tareas, 0);
    assert_eq!(db.precision_estimacion().unwrap().factor(), None);
}

#[test]
fn sin_estimacion_no_hay_nada_que_comparar() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "sin estimar", None);
    suena(&db, t, t0());
    db.completar(t, t0() + Duration::hours(1)).unwrap();

    assert_eq!(db.precision_estimacion().unwrap().tareas, 0);
}

#[test]
fn las_interrupciones_y_el_tiempo_con_claude_entran_en_el_resumen() {
    let db = Db::en_memoria().unwrap();
    let p = proyecto(&db, None, "strapp");
    let t = tarea(&db, p, "una", None);
    suena(&db, t, t0());

    db.registrar_interrupcion(Some(t), None, InterruptionKind::External, t0(), None).unwrap();
    db.registrar_interrupcion(Some(t), None, InterruptionKind::Internal, t0(), None).unwrap();
    db.registrar_interrupcion(Some(t), None, InterruptionKind::Internal, t0(), None).unwrap();
    db.abrir_claude(t, None, "/repo", t0()).unwrap();
    db.cerrar_claude("/repo", t0() + Duration::minutes(20)).unwrap();

    let (d, h) = semana();
    let r = db.resumen_periodo(d, h).unwrap();

    assert_eq!((r.interrupciones_internas, r.interrupciones_externas), (2, 1));
    assert_eq!(r.segundos_con_claude, 1200);
}

#[test]
fn un_periodo_vacio_no_divide_entre_cero() {
    let db = Db::en_memoria().unwrap();
    let r = db.resumen_periodo(t0(), t0() + Duration::days(1)).unwrap();

    assert_eq!(r.pomodoros_completados, 0);
    assert_eq!(r.tasa_de_anulacion(), 0.0);
    assert!(db.carga_por_proyecto(t0(), t0() + Duration::days(1)).unwrap().is_empty());
}
