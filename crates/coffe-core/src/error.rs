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

    #[error("dato corrupto en la base: {0}")]
    DatoCorrupto(String),

    #[error(transparent)]
    Regla(#[from] RuleViolation),
}
