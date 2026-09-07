//! La cuenta que responde "¿me cabe esto?".
//!
//! Un calendario que solo enseña en qué día vence cada cosa no dice nada útil:
//! lo que hace falta saber es si **todo lo que vence de aquí al viernes cabe en
//! los días que quedan**. Eso es una resta, y esta es la resta.
//!
//! Es una función pura: recibe los vencimientos y el día de hoy, y devuelve el
//! plan. No mira el reloj ni la base, así que se puede comprobar un viernes
//! imposible sin esperar al viernes.

use crate::config::Agenda;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

/// Lo que se debe para un día concreto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vencimiento {
    pub fecha: NaiveDate,
    /// Pomodoros que le faltan a lo que vence ese día.
    pub pomodoros: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiaAgenda {
    pub fecha: NaiveDate,
    pub laborable: bool,
    /// Pomodoros que vencen exactamente ese día.
    pub debidos: u32,
    /// Todo lo que vence desde hoy hasta ese día, inclusive. Lo vencido de
    /// antes entra aquí desde el primer día: la deuda no caduca.
    pub deuda_acumulada: u32,
    /// Lo que cabe desde hoy hasta ese día, inclusive.
    pub capacidad_acumulada: u32,
    /// Si lo que se debe para entonces ya no cabe en lo que queda.
    pub imposible: bool,
}

impl DiaAgenda {
    /// Por cuántos pomodoros se pasa. Cero si cabe.
    pub fn exceso(&self) -> u32 {
        self.deuda_acumulada.saturating_sub(self.capacidad_acumulada)
    }
}

/// El plan desde hoy hasta `dias` días por delante.
///
/// Lo que ya venció y sigue sin hacerse **no se reparte hacia atrás**: cae
/// entero sobre hoy. Repartirlo por los días en que se prometió sería contarse
/// un cuento; el trabajo atrasado se hace ahora, no ayer.
pub fn planificar(
    hoy: NaiveDate,
    vencimientos: &[Vencimiento],
    dias: u32,
    cfg: &Agenda,
) -> Vec<DiaAgenda> {
    let atrasado: u32 = vencimientos.iter().filter(|v| v.fecha < hoy).map(|v| v.pomodoros).sum();

    let mut plan = Vec::with_capacity(dias as usize);
    let mut deuda = 0u32;
    let mut capacidad = 0u32;

    for i in 0..dias {
        let fecha = hoy + Duration::days(i as i64);
        let laborable = es_laborable(fecha, cfg);

        let del_dia: u32 =
            vencimientos.iter().filter(|v| v.fecha == fecha).map(|v| v.pomodoros).sum();
        // Lo atrasado se suma el primer día y solo el primero.
        let debidos = if i == 0 { del_dia + atrasado } else { del_dia };

        deuda += debidos;
        if laborable {
            capacidad += cfg.pomodoros_por_dia;
        }

        plan.push(DiaAgenda {
            fecha,
            laborable,
            debidos,
            deuda_acumulada: deuda,
            capacidad_acumulada: capacidad,
            imposible: deuda > capacidad,
        });
    }

    plan
}

fn es_laborable(fecha: NaiveDate, cfg: &Agenda) -> bool {
    cfg.fines_de_semana || !matches!(fecha.weekday(), Weekday::Sat | Weekday::Sun)
}

/// El primer día del plan que ya no cabe, si lo hay. Es la respuesta corta a
/// "¿voy bien?".
pub fn primer_dia_imposible(plan: &[DiaAgenda]) -> Option<&DiaAgenda> {
    plan.iter().find(|d| d.imposible)
}
