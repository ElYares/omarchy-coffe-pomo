//! La cuenta que dice si un día es imposible. Si esta miente, el calendario
//! deja de servir para lo único que sirve.

use chrono::NaiveDate;
use coffe_core::agenda::{Vencimiento, planificar, primer_dia_imposible};
use coffe_core::config::Agenda;

/// Lunes 7 de septiembre de 2026.
fn lunes() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()
}

fn dia(n: i64) -> NaiveDate {
    lunes() + chrono::Duration::days(n)
}

fn vence(n: i64, pomodoros: u32) -> Vencimiento {
    Vencimiento { fecha: dia(n), pomodoros }
}

fn cfg() -> Agenda {
    Agenda::default() // 8 al día, sin fines de semana
}

#[test]
fn una_semana_holgada_no_asusta_a_nadie() {
    let plan = planificar(lunes(), &[vence(2, 6), vence(4, 5)], 7, &cfg());

    assert!(plan.iter().all(|d| !d.imposible));
    assert!(primer_dia_imposible(&plan).is_none());
}

#[test]
fn el_martes_imposible_se_ve_el_lunes() {
    // Veinte pomodoros comprometidos para el martes. En el calendario se ve
    // una sola entrega y parece manejable; la cuenta dice que de lunes a martes
    // solo caben dieciséis.
    let plan = planificar(lunes(), &[vence(1, 20), vence(4, 6)], 7, &cfg());

    let apretado = primer_dia_imposible(&plan).expect("el martes no cabe");
    assert_eq!(apretado.fecha, dia(1));
    assert_eq!(apretado.deuda_acumulada, 20);
    assert_eq!(apretado.capacidad_acumulada, 16);
    assert_eq!(apretado.exceso(), 4, "hay que mover cuatro pomodoros o la fecha");

    // El viernes, con toda la semana por delante, la suma sí cabe: el problema
    // no es el volumen, es CUÁNDO vence.
    let viernes = plan.iter().find(|d| d.fecha == dia(4)).unwrap();
    assert!(!viernes.imposible);
    assert_eq!(viernes.deuda_acumulada, 26);
    assert_eq!(viernes.capacidad_acumulada, 40);
}

#[test]
fn el_fin_de_semana_no_suma_capacidad_por_defecto() {
    // Sábado y domingo son los días 5 y 6.
    let plan = planificar(lunes(), &[], 7, &cfg());

    let sabado = &plan[5];
    let domingo = &plan[6];
    assert!(!sabado.laborable && !domingo.laborable);
    assert_eq!(
        sabado.capacidad_acumulada, domingo.capacidad_acumulada,
        "dos días sin trabajar no añaden ni un pomodoro"
    );
    assert_eq!(sabado.capacidad_acumulada, 40, "solo cuentan los cinco laborables");
}

#[test]
fn quien_trabaja_los_findes_lo_dice_en_la_configuracion() {
    let siempre = Agenda { fines_de_semana: true, ..Agenda::default() };
    let plan = planificar(lunes(), &[], 7, &siempre);

    assert!(plan.iter().all(|d| d.laborable));
    assert_eq!(plan[6].capacidad_acumulada, 56);
}

#[test]
fn lo_atrasado_cae_entero_sobre_hoy() {
    // Repartir el trabajo vencido por los días en que se prometió sería
    // contarse un cuento: esos días ya pasaron y no queda capacidad ahí.
    let plan = planificar(lunes(), &[vence(-3, 5), vence(-10, 4)], 3, &cfg());

    assert_eq!(plan[0].debidos, 9, "lo vencido pesa hoy");
    assert_eq!(plan[0].deuda_acumulada, 9);
    assert_eq!(plan[0].capacidad_acumulada, 8);
    assert!(plan[0].imposible, "nueve pomodoros de deuda no caben en un día de ocho");
    assert_eq!(plan[0].exceso(), 1);

    // Y no se vuelve a contar el resto de días.
    assert_eq!(plan[1].debidos, 0);
    assert_eq!(plan[1].deuda_acumulada, 9);
    assert!(!plan[1].imposible, "con dos días ya cabe");
}

#[test]
fn un_dia_justo_no_es_imposible() {
    // Ocho para hoy con ocho de capacidad: cabe, y no hay que asustar por
    // llegar al límite. Solo pasarse cuenta.
    let plan = planificar(lunes(), &[vence(0, 8)], 2, &cfg());

    assert!(!plan[0].imposible);
    assert_eq!(plan[0].exceso(), 0);

    let plan = planificar(lunes(), &[vence(0, 9)], 2, &cfg());
    assert!(plan[0].imposible);
}

#[test]
fn empezar_en_sabado_no_regala_capacidad() {
    let sabado = NaiveDate::from_ymd_opt(2026, 9, 12).unwrap();
    let plan = planificar(sabado, &[Vencimiento { fecha: sabado, pomodoros: 1 }], 1, &cfg());

    assert!(!plan[0].laborable);
    assert_eq!(plan[0].capacidad_acumulada, 0);
    assert!(plan[0].imposible, "un pomodoro en un día que no se trabaja no cabe");
}

#[test]
fn sin_nada_que_entregar_el_plan_sigue_teniendo_dias() {
    let plan = planificar(lunes(), &[], 30, &cfg());

    assert_eq!(plan.len(), 30);
    assert!(plan.iter().all(|d| d.debidos == 0 && !d.imposible));
    assert_eq!(plan[0].fecha, lunes());
}
