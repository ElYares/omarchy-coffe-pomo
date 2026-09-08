//! La regla de quién mueve una tarea.
//!
//! Vivía en la ventana y se probaba ahí. Ahora la comparten el tablero y la
//! CLI, así que se prueba en el núcleo: si estas pruebas se cumplen, cerrar una
//! tarea desde la terminal y cerrarla arrastrando la tarjeta hacen exactamente
//! lo mismo. Que era el problema.

use coffe_core::model::TaskState;
use coffe_core::tablero::{Destino, Escritura, Movida, OrdenReloj, decidir};

#[test]
fn dejar_la_tarea_donde_ya_estaba_no_hace_nada() {
    assert_eq!(decidir(Destino::EnCurso, 1, true, true, TaskState::InProgress), Movida::Nada);
    assert_eq!(decidir(Destino::Hecha, 1, false, false, TaskState::Done), Movida::Nada);
    assert_eq!(decidir(Destino::Pendiente, 1, false, false, TaskState::Pending), Movida::Nada);
    assert_eq!(decidir(Destino::Archivada, 1, false, false, TaskState::Archived), Movida::Nada);
}

#[test]
fn aparcar_la_que_esta_corriendo_pasa_por_el_reloj() {
    assert_eq!(
        decidir(Destino::Refri, 1, true, true, TaskState::InProgress),
        Movida::AlReloj(OrdenReloj::Pause),
        "hay un pomodoro que anular, y eso no lo puede hacer la base"
    );
}

#[test]
fn al_refri_no_va_lo_que_nunca_empezo() {
    assert!(matches!(
        decidir(Destino::Refri, 1, false, false, TaskState::Pending),
        Movida::Rechazar(_)
    ));
    assert!(matches!(
        decidir(Destino::Refri, 1, false, false, TaskState::Done),
        Movida::Rechazar(_)
    ));
}

#[test]
fn terminar_la_activa_pasa_por_el_reloj_pero_otra_no() {
    // En estricto, terminar la activa NO calla el pomodoro: lo deja sonar. Por
    // eso tiene que ir al daemon y no a la base.
    assert_eq!(
        decidir(Destino::Hecha, 3, true, true, TaskState::InProgress),
        Movida::AlReloj(OrdenReloj::Done)
    );
    assert_eq!(
        decidir(Destino::Hecha, 4, false, true, TaskState::Pending),
        Movida::ALaBase(Escritura::Completar),
        "una tarea que no está bajo el reloj se cierra directo"
    );
}

#[test]
fn no_se_devuelve_a_pendiente_algo_que_esta_corriendo() {
    assert!(matches!(
        decidir(Destino::Pendiente, 1, true, true, TaskState::InProgress),
        Movida::Rechazar(_)
    ));
    assert_eq!(
        decidir(Destino::Pendiente, 1, false, false, TaskState::Done),
        Movida::ALaBase(Escritura::Reabrir)
    );
}

// --- lo que antes no tenía camino ninguno -------------------------------

#[test]
fn cerrar_una_tarea_no_exige_arrancarle_un_pomodoro() {
    // El agujero que motivó todo esto: la única forma de llegar a `done` era
    // tener el reloj encima de esa tarea. Cerrar cinco casillas costaba cinco
    // pomodoros falsos, y arrancar cada uno anulaba el anterior.
    for estado in [TaskState::Pending, TaskState::Paused, TaskState::InProgress] {
        assert_eq!(
            decidir(Destino::Hecha, 9, false, false, estado),
            Movida::ALaBase(Escritura::Completar),
            "con el reloj parado, cerrar {estado:?} se escribe y ya"
        );
    }
}

#[test]
fn cerrar_otra_tarea_no_toca_el_pomodoro_que_corre() {
    // Con un pomodoro vivo sobre la tarea 1, cerrar la 2 no puede pasar por el
    // reloj: haría `Done` sobre la que corre y cerraría la que no era.
    assert_eq!(
        decidir(Destino::Hecha, 2, false, true, TaskState::Pending),
        Movida::ALaBase(Escritura::Completar),
        "el tiempo medido de la tarea 1 no se toca por cerrar la 2"
    );
}

#[test]
fn archivar_es_para_lo_que_ya_no_se_va_a_hacer() {
    for estado in [TaskState::Pending, TaskState::Paused, TaskState::InProgress, TaskState::Done] {
        assert_eq!(
            decidir(Destino::Archivada, 5, false, false, estado),
            Movida::ALaBase(Escritura::Archivar),
            "desde {estado:?} se puede apartar"
        );
    }
}

#[test]
fn no_se_archiva_la_que_tiene_el_reloj_encima() {
    // Quedaría un pomodoro vivo apuntando a algo que ya no se ve, y al sonar
    // escribiría tiempo sobre una tarea archivada.
    assert!(matches!(
        decidir(Destino::Archivada, 1, true, true, TaskState::InProgress),
        Movida::Rechazar(_)
    ));
}

#[test]
fn archivar_no_pasa_nunca_por_el_reloj() {
    // Es la garantía de que apartar trabajo jamás puede costarte un pomodoro
    // medido: ninguna combinación de estado manda archivar al daemon.
    for estado in [
        TaskState::Pending,
        TaskState::InProgress,
        TaskState::Paused,
        TaskState::Done,
        TaskState::Archived,
    ] {
        for corriendo in [false, true] {
            let m = decidir(Destino::Archivada, 7, false, corriendo, estado);
            assert!(
                !matches!(m, Movida::AlReloj(_)),
                "archivar {estado:?} con reloj={corriendo} fue al reloj: {m:?}"
            );
        }
    }
}
