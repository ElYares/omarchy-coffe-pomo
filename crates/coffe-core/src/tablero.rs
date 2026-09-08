//! Adónde puede ir una tarea, y quién la mueve.
//!
//! **El estado de una tarea lo manda el reloj, no la interfaz.** Si el tablero
//! —o la CLI— escribiera el estado por su cuenta, dar por «en curso» algo
//! dejaría una tarea marcada como trabajándose sin ningún pomodoro detrás, y
//! las dos mitades de la aplicación contarían cosas distintas del mismo día.
//!
//! Así que todo lo que toca al reloj se le pide al daemon, y solo lo que no lo
//! toca va directo a la base. Quién decide cuál es cuál es [`decidir`], y vive
//! aquí y no en la ventana porque la CLI necesita **la misma** regla: dos
//! copias se separan, y acabarías pudiendo cerrar desde la terminal una tarea
//! que el tablero no te deja cerrar.
//!
//! Es pura: no mira el reloj del sistema, no abre la base y no habla por el
//! socket. Recibe el estado del mundo y devuelve qué hay que hacer.

use crate::model::TaskState;

/// Adónde se quiere llevar la tarea.
///
/// No es lo mismo que [`TaskState`]: es lo que alguien puede **pedir**, y eso
/// es más de lo que el tablero enseña —`Archivada` no es una columna— y menos
/// de lo que una tarea puede ser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destino {
    Pendiente,
    EnCurso,
    Refri,
    Hecha,
    /// El fondo del bote: trabajo que ya no se va a hacer. Conserva el
    /// historial —por eso no es un borrado— pero sale de la vista.
    Archivada,
}

/// Lo que hay que pedirle al reloj.
///
/// Es un espejo de lo que entiende el daemon, escrito aquí porque el núcleo no
/// depende del protocolo: `coffe-ipc` depende de `coffe-core` y no al revés.
/// Cada quien lo traduce a su `Request` al mandarlo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrdenReloj {
    Start { task_id: i64 },
    Switch { task_id: i64 },
    Pause,
    Done,
}

/// Lo que se escribe directo, porque no toca al reloj.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escritura {
    Aparcar,
    Completar,
    Reabrir,
    Archivar,
}

/// Qué hay que hacer para llevar una tarea a un destino.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Movida {
    /// Ya está donde se pide. No es un error.
    Nada,
    AlReloj(OrdenReloj),
    ALaBase(Escritura),
    /// El destino no admite esa tarea, y hay que decir por qué.
    Rechazar(&'static str),
}

/// La regla entera, comprobable sin ventana, sin daemon y sin base.
///
/// - `es_la_activa`: esta tarea es la que tiene el reloj encima ahora mismo.
/// - `hay_reloj_corriendo`: hay un pomodoro vivo, sea de esta tarea o de otra.
pub fn decidir(
    destino: Destino,
    id: i64,
    es_la_activa: bool,
    hay_reloj_corriendo: bool,
    estado: TaskState,
) -> Movida {
    match destino {
        Destino::EnCurso if es_la_activa => Movida::Nada,
        // Con un pomodoro vivo, empezar otra cosa es un cambio en caliente:
        // anula el actual y lo deja apuntado como tal. No es lo mismo que
        // arrancar en frío y el daemon tiene que saberlo.
        Destino::EnCurso if hay_reloj_corriendo => {
            Movida::AlReloj(OrdenReloj::Switch { task_id: id })
        }
        Destino::EnCurso => Movida::AlReloj(OrdenReloj::Start { task_id: id }),

        Destino::Refri if es_la_activa => Movida::AlReloj(OrdenReloj::Pause),
        Destino::Refri if estado == TaskState::InProgress => Movida::ALaBase(Escritura::Aparcar),
        Destino::Refri => Movida::Rechazar("al refri solo va lo que está en curso"),

        // En estricto esto NO calla el pomodoro: la tarea queda hecha y el
        // reloj sigue hasta sonar. Es la regla, no un descuido.
        Destino::Hecha if es_la_activa => Movida::AlReloj(OrdenReloj::Done),
        Destino::Hecha if estado == TaskState::Done => Movida::Nada,
        Destino::Hecha => Movida::ALaBase(Escritura::Completar),

        // Archivar la que está corriendo la haría desaparecer con el reloj
        // encima: quedaría un pomodoro vivo apuntando a algo que ya no se ve, y
        // al sonar escribiría tiempo sobre una tarea archivada. Se para
        // primero, y así además es el usuario quien decide si ese pomodoro
        // cuenta o se tira.
        Destino::Archivada if es_la_activa => {
            Movida::Rechazar("está corriendo: apárcala, termínala o tira el pomodoro antes")
        }
        Destino::Archivada if estado == TaskState::Archived => Movida::Nada,
        Destino::Archivada => Movida::ALaBase(Escritura::Archivar),

        Destino::Pendiente if es_la_activa => {
            Movida::Rechazar("está corriendo: apárcala o termínala antes")
        }
        Destino::Pendiente if estado == TaskState::Pending => Movida::Nada,
        Destino::Pendiente => Movida::ALaBase(Escritura::Reabrir),
    }
}
