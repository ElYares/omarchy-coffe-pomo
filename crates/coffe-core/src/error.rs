use crate::machine::RuleViolation;

#[derive(Debug, thiserror::Error)]
pub enum CoffeError {
    #[error("error de base de datos: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("no se pudo crear el directorio {ruta}: {fuente}")]
    Directorio {
        ruta: String,
        #[source]
        fuente: std::io::Error,
    },

    #[error(
        "la base está en la versión {encontrada} y este binario solo entiende \
         hasta la {soportada}: actualiza coffe"
    )]
    BaseDelFuturo { encontrada: i64, soportada: i64 },

    #[error("no existe {que} con id {id}")]
    NoExiste { que: &'static str, id: i64 },

    #[error(
        "la tarea {id} ya arrancó ({estado}): la prioridad se congela con el \
         primer pomodoro"
    )]
    PrioridadCongelada { id: i64, estado: &'static str },

    #[error(
        "no puedes colgar el proyecto {id} de {destino}: es su propio \
         descendiente y el arbol se partiria"
    )]
    CicloEnElArbol { id: i64, destino: i64 },

    #[error(
        "el proyecto {id} todavia tiene {hijos} subproyecto(s) y {tareas} \
         tarea(s). Archivalo, o borralo con --force si de verdad quieres \
         perder su historial"
    )]
    ProyectoConContenido { id: i64, hijos: u32, tareas: u32 },

    #[error(
        "la tarea {id} tiene {pomodoros} pomodoro(s) registrados. Borrarla se \
         lleva ese tiempo por delante: usa --force si es lo que quieres"
    )]
    TareaConHistorial { id: i64, pomodoros: u32 },

    #[error("dato corrupto en la base: {0}")]
    DatoCorrupto(String),

    #[error(transparent)]
    Regla(#[from] RuleViolation),
}
