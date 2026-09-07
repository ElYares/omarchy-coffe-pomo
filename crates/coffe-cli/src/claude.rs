//! Los hooks de Claude Code.
//!
//! Miden **cuánto del trabajo se hizo con Claude**, y lo miden de verdad: se
//! abre un tramo cuando Claude empieza a responder y se cierra cuando para. No
//! se estima, no se deduce del historial. Si no había un pomodoro corriendo, no
//! se apunta nada — porque entonces tampoco hay tarea a la que atribuirlo.
//!
//! Corren en **cada mensaje** de cada sesión, así que tienen que ser rápidos y
//! callados: una consulta al daemon y una escritura.

use anyhow::Result;
use chrono::Utc;
use coffe_core::{Db, paths};
use coffe_ipc::{Request, client};

use crate::ClaudeCmd;

pub fn ejecutar(cmd: ClaudeCmd) -> Result<()> {
    let cwd = std::env::current_dir()?.display().to_string();
    let db = Db::open(&paths::database())?;

    match cmd {
        ClaudeCmd::Start => {
            let snap = client::ask(&paths::socket(), &Request::Status)?;

            // Sin reloj corriendo no hay a qué atribuir el tiempo. Arrancar un
            // pomodoro por nuestra cuenta seria peor: el metodo es una decisión
            // del usuario, no algo que le pase por escribirle a Claude.
            let Some(tarea) = snap.task.filter(|_| !snap.state.is_idle()) else {
                return Ok(());
            };

            // Si el directorio es de OTRO proyecto, este tiempo no es de esta
            // tarea: apuntarlo sería mentir en el reporte que este comando
            // existe para poder leer.
            //
            // Un directorio que no es de ningún proyecto sí cuenta. No hay
            // evidencia en contra, y exigir que cada repo esté registrado
            // dejaría la medición vacía justo para quien no ha configurado
            // nada — que es todo el mundo el primer día.
            if let Some(proyecto) = db.proyecto_por_ruta(&cwd)?
                && db.tarea(tarea.id)?.project_id != proyecto.id
            {
                return Ok(());
            }

            let pomodoro = db.pomodoro_abierto()?.map(|(id, _, _)| id);
            db.abrir_claude(tarea.id, pomodoro, &cwd, Utc::now())?;
        }

        ClaudeCmd::Stop => {
            db.cerrar_claude(&cwd, Utc::now())?;
        }
    }
    Ok(())
}
