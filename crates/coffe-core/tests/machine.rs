//! Las reglas del método, uña por uña. Si una de estas falla, la app miente
//! sobre el tiempo del usuario, que es lo único que tiene que hacer bien.

use chrono::{DateTime, Duration, TimeZone, Utc};
use coffe_core::config::Pomodoro;
use coffe_core::machine::{Command, Effect, Machine, RuleViolation, SessionEnd, TimerState};
use coffe_core::model::{BreakKind, InterruptionKind, VoidReason};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
}

fn estricta() -> Machine {
    Machine::new(Pomodoro::default())
}

fn flexible() -> Machine {
    Machine::new(Pomodoro { strict: false, ..Pomodoro::default() })
}

/// Lleva la máquina hasta el final de un pomodoro y de su descanso, dejándola
/// lista para el siguiente. Devuelve el instante en que quedó.
fn ciclo_completo(m: &mut Machine, task: i64, desde: DateTime<Utc>) -> DateTime<Utc> {
    m.apply(Command::Start { task_id: task }, desde).unwrap();
    let tras_foco = desde + Duration::minutes(25);
    m.apply(Command::Tick, tras_foco).unwrap();
    let tras_descanso = tras_foco + Duration::minutes(30);
    m.apply(Command::Tick, tras_descanso).unwrap();
    tras_descanso
}

#[test]
fn arrancar_abre_foco_y_tramo_de_tarea() {
    let mut m = estricta();
    let fx = m.apply(Command::Start { task_id: 7 }, t0()).unwrap();

    assert_eq!(
        fx,
        vec![
            Effect::SessionOpened { task_id: 7 },
            Effect::TaskStarted { task_id: 7 },
            Effect::FocusStarted { task_id: 7, ends_at: t0() + Duration::minutes(25) },
        ]
    );
    assert!(matches!(m.state(), TimerState::Focus { task_id: 7, .. }));
}

#[test]
fn el_pomodoro_al_sonar_abre_descanso_corto() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    let fx = m.apply(Command::Tick, t0() + Duration::minutes(25)).unwrap();

    assert_eq!(fx[0], Effect::FocusCompleted { task_id: 1, at: t0() + Duration::minutes(25) });
    assert!(matches!(fx[1], Effect::BreakStarted { kind: BreakKind::Short, .. }));
    assert_eq!(m.completed_since_long_break(), 1);
}

#[test]
fn el_descanso_largo_llega_al_cuarto_completado() {
    let mut m = estricta();
    let mut ahora = t0();

    for _ in 0..3 {
        ahora = ciclo_completo(&mut m, 1, ahora);
    }
    assert_eq!(m.completed_since_long_break(), 3);
    assert_eq!(m.until_long_break(), 1);

    m.apply(Command::Start { task_id: 1 }, ahora).unwrap();
    let fx = m.apply(Command::Tick, ahora + Duration::minutes(25)).unwrap();

    assert!(
        matches!(fx[1], Effect::BreakStarted { kind: BreakKind::Long, .. }),
        "el cuarto pomodoro tiene que abrir descanso largo, salió {:?}",
        fx[1]
    );
    assert_eq!(m.completed_since_long_break(), 0, "el contador se reinicia");
}

#[test]
fn los_anulados_no_acercan_el_descanso_largo() {
    let mut m = estricta();
    let mut ahora = t0();

    for _ in 0..6 {
        m.apply(Command::Start { task_id: 1 }, ahora).unwrap();
        ahora += Duration::minutes(10);
        m.apply(Command::Void { reason: VoidReason::Interrupted }, ahora).unwrap();
        ahora += Duration::minutes(1);
    }

    assert_eq!(m.completed_since_long_break(), 0, "seis pomodoros tirados no valen ni uno");
}

#[test]
fn pausar_a_media_anula_el_pomodoro_y_manda_la_tarea_al_refri() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 4 }, t0()).unwrap();

    let fx = m.apply(Command::Pause, t0() + Duration::minutes(11)).unwrap();

    assert_eq!(
        fx,
        vec![
            Effect::FocusVoided {
                task_id: 4,
                reason: VoidReason::Paused,
                elapsed_secs: 660,
                at: t0() + Duration::minutes(11),
            },
            Effect::SessionClosed {
                task_id: 4,
                reason: SessionEnd::Parked,
                at: t0() + Duration::minutes(11),
            },
            Effect::TaskParked { task_id: 4 },
        ]
    );
    assert!(m.state().is_idle());
    assert_eq!(m.completed_since_long_break(), 0);
}

#[test]
fn al_retomar_se_sirve_taza_nueva_no_la_de_antes() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 4 }, t0()).unwrap();
    m.apply(Command::Pause, t0() + Duration::minutes(20)).unwrap();

    let vuelta = t0() + Duration::hours(2);
    m.apply(Command::Start { task_id: 4 }, vuelta).unwrap();

    assert_eq!(
        m.state().remaining(vuelta),
        Duration::minutes(25),
        "el pomodoro no se reanuda a los cinco minutos que quedaban"
    );
}

#[test]
fn anular_no_aparca_la_tarea() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 9 }, t0()).unwrap();

    let fx = m.apply(Command::Void { reason: VoidReason::Abandoned }, t0()).unwrap();

    assert!(matches!(fx[0], Effect::FocusVoided { .. }));
    // El tramo se cierra porque el reloj se paró, pero como `idle`: tirar el
    // pomodoro no es aparcar la tarea, y no debe contar como pausa.
    assert_eq!(fx[1], Effect::SessionClosed { task_id: 9, reason: SessionEnd::Idle, at: t0() });
    assert!(!fx.iter().any(|e| matches!(e, Effect::TaskParked { .. })));
}

#[test]
fn el_descanso_bloquea_el_foco_siguiente_en_estricto() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    let sono = t0() + Duration::minutes(25);
    m.apply(Command::Tick, sono).unwrap();

    let err = m.apply(Command::Start { task_id: 2 }, sono + Duration::minutes(1)).unwrap_err();

    assert_eq!(err, RuleViolation::BreakInProgress { ends_at: sono + Duration::minutes(5) });
}

#[test]
fn el_descanso_no_se_salta_en_estricto() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    let sono = t0() + Duration::minutes(25);
    m.apply(Command::Tick, sono).unwrap();

    assert_eq!(m.apply(Command::SkipBreak, sono).unwrap_err(), RuleViolation::BreakMandatory);
}

#[test]
fn fuera_de_estricto_el_descanso_si_se_corta() {
    let mut m = flexible();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    let sono = t0() + Duration::minutes(25);
    m.apply(Command::Tick, sono).unwrap();

    let fx = m.apply(Command::Start { task_id: 2 }, sono).unwrap();

    assert_eq!(fx[0], Effect::BreakEnded { kind: BreakKind::Short, at: sono });
    assert!(matches!(m.state(), TimerState::Focus { task_id: 2, .. }));
}

#[test]
fn terminar_la_tarea_no_calla_el_pomodoro_en_estricto() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 3 }, t0()).unwrap();

    let fx = m.apply(Command::Done, t0() + Duration::minutes(9)).unwrap();

    assert_eq!(fx, vec![Effect::TaskCompleted { task_id: 3 }]);
    assert!(
        matches!(m.state(), TimerState::Focus { overlearning: true, .. }),
        "si un pomodoro empieza, tiene que sonar: el resto es para repasar"
    );

    // Y cuando suena, cuenta como completado.
    let fx = m.apply(Command::Tick, t0() + Duration::minutes(25)).unwrap();
    assert_eq!(fx[0], Effect::FocusCompleted { task_id: 3, at: t0() + Duration::minutes(25) });
    assert_eq!(m.completed_since_long_break(), 1);
}

#[test]
fn fuera_de_estricto_terminar_la_tarea_cierra_el_pomodoro() {
    let mut m = flexible();
    m.apply(Command::Start { task_id: 3 }, t0()).unwrap();

    let fx = m.apply(Command::Done, t0() + Duration::minutes(9)).unwrap();

    assert!(matches!(fx[0], Effect::FocusVoided { reason: VoidReason::FinishedEarly, .. }));
    assert_eq!(
        fx[1],
        Effect::SessionClosed {
            task_id: 3,
            reason: SessionEnd::Done,
            at: t0() + Duration::minutes(9)
        }
    );
    assert_eq!(fx[2], Effect::TaskCompleted { task_id: 3 });
    assert!(m.state().is_idle());
}

#[test]
fn cambiar_de_tarea_anula_aparca_la_vieja_y_abre_la_nueva() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    let cambio = t0() + Duration::minutes(7);
    let fx = m.apply(Command::Switch { task_id: 2 }, cambio).unwrap();

    assert!(matches!(fx[0], Effect::FocusVoided { task_id: 1, reason: VoidReason::Switched, .. }));
    assert_eq!(
        fx[1],
        Effect::SessionClosed { task_id: 1, reason: SessionEnd::Switched, at: cambio }
    );
    assert_eq!(fx[2], Effect::TaskParked { task_id: 1 });
    assert_eq!(fx[3], Effect::SessionOpened { task_id: 2 });
    assert!(matches!(m.state(), TimerState::Focus { task_id: 2, .. }));
}

#[test]
fn la_interrupcion_se_apunta_pero_no_corta() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 5 }, t0()).unwrap();

    let fx = m
        .apply(Command::Interrupt { kind: InterruptionKind::External }, t0() + Duration::minutes(3))
        .unwrap();

    assert_eq!(
        fx,
        vec![Effect::InterruptionLogged { task_id: Some(5), kind: InterruptionKind::External }]
    );
    assert!(matches!(m.state(), TimerState::Focus { .. }), "informar y volver cabe dentro");
}

#[test]
fn un_tick_atrasado_dentro_de_la_gracia_hace_sonar_el_pomodoro() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    // El daemon tardó un minuto en despertar. Un minuto no invalida nada.
    let tarde = t0() + Duration::minutes(26);
    let fx = m.apply(Command::Tick, tarde).unwrap();

    assert_eq!(
        fx[0],
        Effect::FocusCompleted { task_id: 1, at: t0() + Duration::minutes(25) },
        "y se apunta en el minuto 25, no en el 26"
    );
    assert!(matches!(fx[1], Effect::BreakStarted { .. }));
    assert_eq!(m.completed_since_long_break(), 1);
}

#[test]
fn un_pomodoro_que_sono_con_la_maquina_dormida_no_se_regala() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    // Se cerró la tapa a los diez minutos y se abrió hora y media después.
    let fx = m.apply(Command::Tick, t0() + Duration::minutes(90)).unwrap();

    assert_eq!(
        fx[0],
        Effect::FocusVoided {
            task_id: 1,
            reason: VoidReason::DaemonLost,
            elapsed_secs: 1500,
            at: t0() + Duration::minutes(25),
        },
        "la campana no la oyó nadie: el pomodoro se anula"
    );
    assert_eq!(
        fx[1],
        Effect::SessionClosed {
            task_id: 1,
            reason: SessionEnd::Recovered,
            at: t0() + Duration::minutes(25)
        }
    );
    assert!(m.state().is_idle(), "y no se abre un descanso que nadie pidió");
    assert_eq!(m.completed_since_long_break(), 0);
}
#[test]
fn un_comando_atrasado_ve_primero_el_reloj() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    // Este `start` llega a los 40 minutos: el foco de antes ya no existe, y
    // como nadie oyó su campana, se anula en vez de contar.
    let fx = m.apply(Command::Start { task_id: 2 }, t0() + Duration::minutes(40)).unwrap();

    assert!(matches!(
        fx[0],
        Effect::FocusVoided { task_id: 1, reason: VoidReason::DaemonLost, .. }
    ));
    assert!(matches!(m.state(), TimerState::Focus { task_id: 2, .. }));
}
#[test]
fn no_se_encima_un_foco_sobre_otro() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    let err = m.apply(Command::Start { task_id: 2 }, t0() + Duration::minutes(3)).unwrap_err();

    assert_eq!(err, RuleViolation::AlreadyFocused { task_id: 1 });
}

#[test]
fn sin_nada_corriendo_no_hay_nada_que_pausar() {
    let mut m = estricta();
    assert_eq!(m.apply(Command::Pause, t0()).unwrap_err(), RuleViolation::NothingRunning);
    assert_eq!(
        m.apply(Command::Void { reason: VoidReason::Abandoned }, t0()).unwrap_err(),
        RuleViolation::NothingRunning
    );
    assert_eq!(m.apply(Command::Done, t0()).unwrap_err(), RuleViolation::NoTaskInProgress);
}

#[test]
fn el_nivel_de_la_taza_va_de_lleno_a_vacio() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    assert_eq!(m.state().progress(t0()), 0.0);
    assert!((m.state().progress(t0() + Duration::seconds(750)) - 0.5).abs() < 1e-9);
    assert_eq!(m.state().progress(t0() + Duration::minutes(25)), 1.0);
    assert_eq!(
        m.state().progress(t0() + Duration::hours(3)),
        1.0,
        "pasarse de la hora no desborda la taza"
    );
}

#[test]
fn el_tiempo_restante_nunca_es_negativo() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();

    assert_eq!(m.state().remaining(t0() + Duration::hours(5)), Duration::zero());
    assert_eq!(TimerState::Idle.remaining(t0()), Duration::zero());
}

#[test]
fn la_maquina_se_reconstruye_tal_como_estaba() {
    let mut m = estricta();
    let mut ahora = t0();
    for _ in 0..2 {
        ahora = ciclo_completo(&mut m, 1, ahora);
    }
    m.apply(Command::Start { task_id: 8 }, ahora).unwrap();

    let json = serde_json::to_string(m.state()).unwrap();
    let estado: TimerState = serde_json::from_str(&json).unwrap();
    let mut vuelta = Machine::restore(Pomodoro::default(), estado, m.completed_since_long_break());

    // El foco que estaba vivo sigue vivo y suena cuando le toca.
    let fx = vuelta.apply(Command::Tick, ahora + Duration::minutes(25)).unwrap();
    assert_eq!(fx[0], Effect::FocusCompleted { task_id: 8, at: ahora + Duration::minutes(25) });
    assert!(
        matches!(fx[1], Effect::BreakStarted { kind: BreakKind::Short, .. }),
        "el tercero sigue siendo tercero tras reiniciar el daemon"
    );
}

#[test]
fn el_tramo_se_cierra_al_acabar_el_ciclo_no_al_dia_siguiente() {
    // Sin esto, olvidarse de aparcar el viernes daría un tramo de tres días y
    // el tiempo de dedicación sería basura.
    let mut m = estricta();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    m.apply(Command::Tick, t0() + Duration::minutes(25)).unwrap();

    let fin_descanso = t0() + Duration::minutes(30);
    let fx = m.apply(Command::Tick, fin_descanso).unwrap();

    assert_eq!(fx[0], Effect::BreakEnded { kind: BreakKind::Short, at: fin_descanso });
    assert_eq!(
        fx[1],
        Effect::SessionClosed { task_id: 1, reason: SessionEnd::Idle, at: fin_descanso },
        "al volver el reloj a cero, el tramo se cierra solo"
    );
    assert!(m.state().is_idle());
}
#[test]
fn el_repaso_cuenta_dentro_del_tramo_y_lo_cierra_al_sonar() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 3 }, t0()).unwrap();
    m.apply(Command::Done, t0() + Duration::minutes(9)).unwrap();

    let fx = m.apply(Command::Tick, t0() + Duration::minutes(25)).unwrap();

    assert_eq!(fx[0], Effect::FocusCompleted { task_id: 3, at: t0() + Duration::minutes(25) });
    assert_eq!(
        fx[1],
        Effect::SessionClosed {
            task_id: 3,
            reason: SessionEnd::Done,
            at: t0() + Duration::minutes(25)
        },
        "los 16 minutos de repaso son tiempo de la tarea"
    );
    // El descanso ya no cuelga de la tarea: está hecha.
    assert!(matches!(m.state(), TimerState::Break { after_task: None, .. }));
}

#[test]
fn marcar_hecha_dos_veces_no_la_termina_dos_veces() {
    let mut m = estricta();
    m.apply(Command::Start { task_id: 3 }, t0()).unwrap();
    m.apply(Command::Done, t0() + Duration::minutes(5)).unwrap();

    assert_eq!(
        m.apply(Command::Done, t0() + Duration::minutes(6)).unwrap_err(),
        RuleViolation::NoTaskInProgress
    );
}

#[test]
fn arrancar_otra_tarea_durante_el_descanso_aparca_la_anterior() {
    let mut m = flexible();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    let sono = t0() + Duration::minutes(25);
    m.apply(Command::Tick, sono).unwrap();

    let fx = m.apply(Command::Start { task_id: 2 }, sono).unwrap();

    assert_eq!(fx[0], Effect::BreakEnded { kind: BreakKind::Short, at: sono });
    assert_eq!(fx[1], Effect::SessionClosed { task_id: 1, reason: SessionEnd::Switched, at: sono });
    assert_eq!(fx[2], Effect::TaskParked { task_id: 1 });
    assert_eq!(fx[3], Effect::SessionOpened { task_id: 2 });
}

#[test]
fn retomar_la_misma_tarea_durante_su_descanso_no_la_aparca() {
    let mut m = flexible();
    m.apply(Command::Start { task_id: 1 }, t0()).unwrap();
    let sono = t0() + Duration::minutes(25);
    m.apply(Command::Tick, sono).unwrap();

    let fx = m.apply(Command::Start { task_id: 1 }, sono).unwrap();

    assert!(
        !fx.iter().any(|e| matches!(e, Effect::TaskParked { .. })),
        "seguir con lo mismo no es aparcarlo: {fx:?}"
    );
}
