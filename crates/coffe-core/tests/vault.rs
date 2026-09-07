//! El lector del vault, contra las formas que de verdad hay en el vault y no
//! contra las que dice la convención. Los ejemplos salen de mirar los 124
//! archivos reales del Backlog.

use coffe_core::model::Priority;
use coffe_core::vault::{Destino, destino, es_terminal, estado_conocido, parsear};

fn nota(fm: &str) -> String {
    format!("---\n{fm}\n---\n\n# Título\n\nCuerpo.\n")
}

#[test]
fn lee_una_historia_normal() {
    let n = parsear(
        "devherd - HU-001 - cooldown de alertas.md",
        &nota("type: historia-usuario\nstatus: propuesta\nproject: devherd\nid: HU-001\nprioridad: alta"),
    )
    .unwrap();

    assert_eq!(n.id, "HU-001");
    assert_eq!(n.titulo, "HU-001 - cooldown de alertas");
    assert_eq!(n.prioridad, Some(Priority::High));
    assert!(!n.terminal);
    assert!(n.estado_conocido);
}

#[test]
fn el_nombre_del_proyecto_no_entra_en_el_titulo() {
    // Repetirlo en cada tarjeta gasta el ancho que necesita el título, y en
    // coffe la tarea ya cuelga de su proyecto.
    let n = parsear(
        "DocOps - CU-001 - Errores observados como candidatos.md",
        &nota("status: propuesta\nid: CU-001"),
    )
    .unwrap();
    assert_eq!(n.titulo, "CU-001 - Errores observados como candidatos");
}

#[test]
fn un_proyecto_con_guiones_no_parte_el_titulo_por_donde_no_es() {
    // Cortar por la primera raya dejaría "mas-server - HU-014 - ...".
    let n = parsear(
        "tl-mas-server - HU-014 - Conciliacion de pagos.md",
        &nota("status: lista\nid: HU-014"),
    )
    .unwrap();
    assert_eq!(n.titulo, "HU-014 - Conciliacion de pagos");
}

#[test]
fn las_veintisiete_notas_sin_prioridad_no_se_pierden() {
    // Casi una de cada cuatro del vault real no trae `prioridad`.
    let n = parsear("x - HU-002 - Algo.md", &nota("status: propuesta\nid: HU-002")).unwrap();

    assert_eq!(n.prioridad, None, "no se inventa: quien importa decide");
    assert_eq!(destino(&n, false), Destino::Importar);
}

#[test]
fn los_estados_terminales_se_reconocen_todos() {
    for e in ["hecha", "descartada", "implementada", "cerrado", "HECHA", " hecha "] {
        assert!(es_terminal(e), "{e:?} debería ser terminal");
    }
    for e in ["propuesta", "lista", "en curso", "en-pruebas", "bloqueado"] {
        assert!(!es_terminal(e), "{e:?} sigue siendo trabajo");
        assert!(estado_conocido(e));
    }
}

#[test]
fn un_estado_desconocido_entra_como_pendiente_y_se_delata() {
    // Dar por hecha una historia por no reconocer su estado sería la peor
    // equivocación posible: desaparece del tablero sin que nadie lo note.
    let n =
        parsear("x - HU-003 - Algo.md", &nota("status: en-revision-externa\nid: HU-003")).unwrap();

    assert!(!n.terminal, "ante la duda, es trabajo");
    assert!(!n.estado_conocido, "y hay que decirlo");
    assert_eq!(destino(&n, false), Destino::Importar);
}

#[test]
fn lo_terminado_se_omite_salvo_que_se_pida() {
    let n = parsear("x - HU-004 - Algo.md", &nota("status: hecha\nid: HU-004")).unwrap();

    assert_eq!(destino(&n, false), Destino::Omitir);
    assert_eq!(destino(&n, true), Destino::Importar);
}

#[test]
fn una_nota_sin_frontmatter_o_sin_id_no_es_una_historia() {
    // En un Backlog puede haber un índice o un borrador; no es un error.
    assert!(parsear("indice.md", "# Índice\n\n- una cosa\n").is_none());
    assert!(parsear("x.md", &nota("status: propuesta\nproject: x")).is_none());
}

#[test]
fn se_leen_la_estimacion_y_la_entrega_si_alguien_las_pone() {
    // La convención del vault no las tiene. Se leen por si se añaden, porque
    // sin estimación la tarea no pesa en el calendario.
    let n = parsear(
        "x - HU-005 - Algo.md",
        &nota("status: propuesta\nid: HU-005\npomodoros: 4\nentrega: 2026-09-11"),
    )
    .unwrap();

    assert_eq!(n.pomodoros, Some(4));
    assert_eq!(n.entrega.as_deref(), Some("2026-09-11"));

    let sin = parsear("x - HU-006 - Algo.md", &nota("status: propuesta\nid: HU-006")).unwrap();
    assert_eq!(sin.pomodoros, None);
    assert_eq!(sin.entrega, None);
}

#[test]
fn una_fecha_a_medias_no_pasa_por_fecha() {
    let n =
        parsear("x - HU-007 - Algo.md", &nota("status: propuesta\nid: HU-007\nentrega: 2026-09"))
            .unwrap();
    assert_eq!(n.entrega, None);
}

#[test]
fn las_comillas_del_frontmatter_no_se_cuelan_en_los_valores() {
    let n = parsear(
        "x - HU-008 - Algo.md",
        &nota("status: \"propuesta\"\nid: 'HU-008'\nprioridad: \"baja\""),
    )
    .unwrap();

    assert_eq!(n.id, "HU-008");
    assert_eq!(n.estado, "propuesta");
    assert_eq!(n.prioridad, Some(Priority::Low));
}

// ---------------------------------------------------- traerlo al tablero

use chrono::{Duration, TimeZone, Utc};
use coffe_core::db::Db;
use coffe_core::db::projects::NuevoProyecto;
use coffe_core::model::TaskState;
use coffe_core::vault::{Nota, NotaHallada};

fn t0() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
}

fn hallada(id: &str, titulo: &str, estado: &str, prio: Option<Priority>) -> NotaHallada {
    NotaHallada {
        ruta: format!("10 Projects/devherd/Backlog/devherd - {id} - {titulo}.md"),
        nota: Nota {
            id: id.into(),
            titulo: format!("{id} - {titulo}"),
            estado: estado.into(),
            prioridad: prio,
            terminal: es_terminal(estado),
            estado_conocido: estado_conocido(estado),
            pomodoros: None,
            entrega: None,
        },
    }
}

fn base() -> (Db, i64) {
    let db = Db::en_memoria().unwrap();
    let p = db
        .crear_proyecto(
            NuevoProyecto { parent_id: None, name: "devherd".into(), ..Default::default() },
            t0(),
        )
        .unwrap();
    (db, p)
}

#[test]
fn importar_dos_veces_no_duplica_nada() {
    // Es lo que hace que se pueda correr el importador cuando a uno le apetezca
    // en vez de solo la primera vez.
    let (db, p) = base();
    let notas = vec![
        hallada("HU-001", "cooldown", "propuesta", Some(Priority::High)),
        hallada("HU-002", "relleno", "lista", Some(Priority::Low)),
    ];

    let r1 = db.sincronizar_notas(p, &notas, false, t0()).unwrap();
    assert_eq!((r1.creadas, r1.actualizadas), (2, 0));

    let r2 = db.sincronizar_notas(p, &notas, false, t0()).unwrap();
    assert_eq!((r2.creadas, r2.actualizadas), (0, 2));
    assert_eq!(db.tareas(&Default::default()).unwrap().len(), 2);
}

#[test]
fn el_vault_manda_sobre_el_titulo_pero_no_sobre_el_estado() {
    // El vault sabe QUÉ hay que hacer; coffe sabe qué se está haciendo. Si una
    // importación devolviera a pendiente una tarea en curso, el reloj y el
    // tablero dejarían de contar lo mismo.
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "viejo", "propuesta", None)], false, t0()).unwrap();
    let id = db.tareas(&Default::default()).unwrap()[0].id;
    db.marcar_en_curso(id, t0()).unwrap();

    db.sincronizar_notas(p, &[hallada("HU-001", "nuevo", "propuesta", None)], false, t0()).unwrap();

    let t = db.tarea(id).unwrap();
    assert_eq!(t.title, "HU-001 - nuevo", "el título sí se refresca");
    assert_eq!(t.state, TaskState::InProgress, "el estado no lo toca nadie desde el vault");
}

#[test]
fn el_vault_no_reescribe_la_prioridad_de_algo_ya_trabajado() {
    let (db, p) = base();
    db.sincronizar_notas(
        p,
        &[hallada("HU-001", "x", "propuesta", Some(Priority::High))],
        false,
        t0(),
    )
    .unwrap();
    let id = db.tareas(&Default::default()).unwrap()[0].id;
    db.marcar_en_curso(id, t0()).unwrap();

    let r = db
        .sincronizar_notas(
            p,
            &[hallada("HU-001", "x", "propuesta", Some(Priority::Low))],
            false,
            t0(),
        )
        .unwrap();

    assert_eq!(db.tarea(id).unwrap().priority, Priority::High);
    assert_eq!(r.prioridad_congelada, vec!["HU-001 - x"], "y se dice, no se traga en silencio");
}

#[test]
fn la_prioridad_si_se_actualiza_mientras_no_haya_arrancado() {
    let (db, p) = base();
    db.sincronizar_notas(
        p,
        &[hallada("HU-001", "x", "propuesta", Some(Priority::Low))],
        false,
        t0(),
    )
    .unwrap();

    db.sincronizar_notas(
        p,
        &[hallada("HU-001", "x", "propuesta", Some(Priority::High))],
        false,
        t0(),
    )
    .unwrap();

    assert_eq!(db.tareas(&Default::default()).unwrap()[0].priority, Priority::High);
}

#[test]
fn lo_hecho_en_el_vault_no_entra_al_tablero() {
    let (db, p) = base();
    let notas = vec![
        hallada("HU-001", "viva", "propuesta", None),
        hallada("HU-002", "hecha", "hecha", None),
        hallada("HU-003", "tirada", "descartada", None),
    ];

    let r = db.sincronizar_notas(p, &notas, false, t0()).unwrap();

    assert_eq!(r.creadas, 1);
    assert_eq!(r.omitidas, 2);
}

#[test]
fn la_importacion_delata_lo_que_no_entiende() {
    let (db, p) = base();
    let notas = vec![
        hallada("HU-001", "rara", "en-revision-externa", Some(Priority::High)),
        hallada("HU-002", "sin prio", "propuesta", None),
    ];

    let r = db.sincronizar_notas(p, &notas, false, t0()).unwrap();

    assert_eq!(r.creadas, 2, "las dos entran: ante la duda son trabajo");
    assert_eq!(
        r.estados_raros,
        vec![("HU-001 - rara".to_string(), "en-revision-externa".to_string())]
    );
    assert_eq!(r.sin_prioridad, vec!["HU-002 - sin prio"]);
    assert_eq!(
        db.tareas(&Default::default()).unwrap()[1].priority,
        Priority::Medium,
        "media y no alta: si todo lo sin etiqueta fuera urgente, nada lo sería"
    );
}

#[test]
fn una_tarea_recuerda_de_que_nota_vino() {
    let (db, p) = base();
    let n = hallada("HU-001", "x", "propuesta", None);
    db.sincronizar_notas(p, std::slice::from_ref(&n), false, t0()).unwrap();

    let t = db.tarea_por_vault(p, "HU-001").unwrap().expect("se encuentra por su ID");
    assert_eq!(t.vault_note.as_deref(), Some(n.ruta.as_str()), "y sabe volver al archivo");
    assert!(db.tarea_por_vault(p, "HU-999").unwrap().is_none());
}

#[test]
fn renombrar_la_nota_actualiza_la_tarea_en_vez_de_duplicarla() {
    // Arreglar una errata en el título de un HU es rutina. Con la ruta del
    // archivo como identidad, la siguiente importación creaba una tarea nueva
    // y dejaba la vieja huérfana con todo su tiempo medido dentro.
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "cooldwon", "propuesta", None)], false, t0())
        .unwrap();

    let renombrada = hallada("HU-001", "cooldown", "propuesta", None);
    let r = db.sincronizar_notas(p, std::slice::from_ref(&renombrada), false, t0()).unwrap();

    assert_eq!((r.creadas, r.actualizadas), (0, 1));
    let tareas = db.tareas(&Default::default()).unwrap();
    assert_eq!(tareas.len(), 1, "una sola tarea, no dos");
    assert_eq!(tareas[0].title, "HU-001 - cooldown");
    assert_eq!(tareas[0].vault_note.as_deref(), Some(renombrada.ruta.as_str()));
}

#[test]
fn el_mismo_id_en_dos_proyectos_son_dos_tareas() {
    // Casi todos los backlogs empiezan en HU-001.
    let (db, a) = base();
    let b = db
        .crear_proyecto(
            NuevoProyecto { parent_id: None, name: "otro".into(), ..Default::default() },
            t0(),
        )
        .unwrap();

    db.sincronizar_notas(a, &[hallada("HU-001", "una", "propuesta", None)], false, t0()).unwrap();
    db.sincronizar_notas(b, &[hallada("HU-001", "otra", "propuesta", None)], false, t0()).unwrap();

    assert_eq!(db.tareas(&Default::default()).unwrap().len(), 2);
}

#[test]
fn una_nota_borrada_del_vault_no_borra_su_tarea() {
    // El tiempo medido no puede desaparecer porque alguien reorganice el vault.
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "x", "propuesta", None)], false, t0()).unwrap();

    let r = db.sincronizar_notas(p, &[], false, t0() + Duration::days(1)).unwrap();

    assert_eq!((r.creadas, r.actualizadas, r.omitidas), (0, 0, 0));
    assert_eq!(db.tareas(&Default::default()).unwrap().len(), 1);
}

// ------------------------------------------------------- tramos de Claude

#[test]
fn un_tramo_de_claude_suma_al_resumen_de_la_tarea() {
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "x", "propuesta", None)], false, t0()).unwrap();
    let id = db.tareas(&Default::default()).unwrap()[0].id;

    assert!(db.abrir_claude(id, None, "/repo", t0()).unwrap());
    let dur = db.cerrar_claude("/repo", t0() + Duration::minutes(12)).unwrap();

    assert_eq!(dur, Some(720));
    assert_eq!(db.resumen_tarea(id).unwrap().segundos_con_claude, 720);
}

#[test]
fn los_hooks_se_disparan_en_cada_mensaje_y_eso_no_duplica_el_tramo() {
    // `start` llega una vez por mensaje, no una por sesión. Si cada uno abriera
    // un tramo, una conversación larga multiplicaría el tiempo medido.
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "x", "propuesta", None)], false, t0()).unwrap();
    let id = db.tareas(&Default::default()).unwrap()[0].id;

    assert!(db.abrir_claude(id, None, "/repo", t0()).unwrap());
    assert!(!db.abrir_claude(id, None, "/repo", t0() + Duration::minutes(2)).unwrap());
    db.cerrar_claude("/repo", t0() + Duration::minutes(10)).unwrap();

    assert_eq!(
        db.resumen_tarea(id).unwrap().segundos_con_claude,
        600,
        "un solo tramo, desde el primer mensaje"
    );
}

#[test]
fn dos_sesiones_de_claude_en_dos_repos_no_se_pisan() {
    // Trabajar con varias sesiones a la vez es lo normal, no un error.
    let (db, p) = base();
    let notas = vec![
        hallada("HU-001", "una", "propuesta", None),
        hallada("HU-002", "otra", "propuesta", None),
    ];
    db.sincronizar_notas(p, &notas, false, t0()).unwrap();
    let ids: Vec<i64> = db.tareas(&Default::default()).unwrap().iter().map(|t| t.id).collect();

    db.abrir_claude(ids[0], None, "/repo-a", t0()).unwrap();
    db.abrir_claude(ids[1], None, "/repo-b", t0()).unwrap();
    db.cerrar_claude("/repo-a", t0() + Duration::minutes(5)).unwrap();
    db.cerrar_claude("/repo-b", t0() + Duration::minutes(9)).unwrap();

    assert_eq!(db.resumen_tarea(ids[0]).unwrap().segundos_con_claude, 300);
    assert_eq!(db.resumen_tarea(ids[1]).unwrap().segundos_con_claude, 540);
}

#[test]
fn cerrar_un_tramo_que_no_existe_no_es_un_error() {
    // El hook de `stop` se dispara aunque el de `start` no llegara a apuntar
    // nada, porque no hubiera pomodoro corriendo.
    let (db, _) = base();
    assert_eq!(db.cerrar_claude("/repo", t0()).unwrap(), None);
}

#[test]
fn un_tramo_abandonado_no_cuenta_para_siempre() {
    // Una sesión de Claude que muere de golpe deja el tramo abierto. Sin esto,
    // el siguiente resumen diría que Claude lleva tres días trabajando.
    let (db, p) = base();
    db.sincronizar_notas(p, &[hallada("HU-001", "x", "propuesta", None)], false, t0()).unwrap();
    let id = db.tareas(&Default::default()).unwrap()[0].id;
    db.abrir_claude(id, None, "/repo", t0()).unwrap();

    let n = db.cerrar_claude_huerfanos(t0() + Duration::days(3)).unwrap();

    assert_eq!(n, 1);
    assert_eq!(
        db.resumen_tarea(id).unwrap().segundos_con_claude,
        0,
        "se cierra donde empezó: no sabemos cuánto duró, así que no inventamos"
    );
}
