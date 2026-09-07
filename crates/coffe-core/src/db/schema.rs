//! El esquema y sus migraciones.
//!
//! Se versiona con `PRAGMA user_version`: cada migración es una función que
//! sube un escalón. Nunca se edita una migración ya publicada; se añade otra.

use rusqlite::Connection;

use crate::error::CoffeError;

/// Cada entrada es un escalón. El índice + 1 es la versión que deja puesta.
const MIGRACIONES: &[&str] = &[
    // v1 — el esquema inicial.
    r#"
    -- Los proyectos son un árbol: strapp > tl-mas, personal > labs > video.
    -- La profundidad no está limitada a propósito.
    CREATE TABLE projects (
        id          INTEGER PRIMARY KEY,
        parent_id   INTEGER REFERENCES projects(id) ON DELETE CASCADE,
        name        TEXT    NOT NULL,
        slug        TEXT    NOT NULL,
        -- Si el proyecto vive en disco, de aquí sale la detección por cwd.
        repo_path   TEXT,
        -- La carpeta del proyecto dentro del vault de Obsidian.
        vault_path  TEXT,
        archived    INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT    NOT NULL
    );

    -- SQLite considera distintos dos NULL, así que un UNIQUE normal dejaría
    -- meter dos raíces con el mismo slug. El COALESCE cierra ese hueco.
    CREATE UNIQUE INDEX projects_slug_por_padre
        ON projects(COALESCE(parent_id, 0), slug);
    CREATE INDEX projects_por_padre ON projects(parent_id);

    CREATE TABLE tasks (
        id                 INTEGER PRIMARY KEY,
        project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        title              TEXT    NOT NULL,
        notes              TEXT,
        priority           TEXT    NOT NULL
                           CHECK (priority IN ('low','medium','high')),
        state              TEXT    NOT NULL
                           CHECK (state IN ('pending','in_progress','paused','done','archived')),
        -- Cirillo estima en pomodoros, no en horas.
        estimate_pomodoros INTEGER,
        due_date           TEXT,
        -- Ruta al HU del vault, relativa a su raíz.
        vault_note         TEXT,
        position           INTEGER NOT NULL DEFAULT 0,
        created_at         TEXT    NOT NULL,
        -- La primera vez que se trabajó. Con completed_at da el tiempo de
        -- calendario, que es distinto del tiempo efectivo.
        first_started_at   TEXT,
        completed_at       TEXT,
        archived_at        TEXT
    );

    CREATE INDEX tasks_por_proyecto ON tasks(project_id);
    CREATE INDEX tasks_por_estado   ON tasks(state);
    CREATE INDEX tasks_por_entrega  ON tasks(due_date) WHERE due_date IS NOT NULL;

    CREATE TABLE pomodoros (
        id           INTEGER PRIMARY KEY,
        task_id      INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        started_at   TEXT    NOT NULL,
        ended_at     TEXT,
        -- NULL mientras corre. 'voided' no cuenta para ninguna métrica de
        -- tiempo efectivo, pero se guarda: saber cuántos se tiran es la mitad
        -- del valor del método.
        outcome      TEXT    CHECK (outcome IN ('completed','voided')),
        void_reason  TEXT,
        -- Si se hizo bajo las reglas estrictas. Los flexibles no son canónicos.
        strict       INTEGER NOT NULL DEFAULT 1,
        planned_secs INTEGER NOT NULL
    );

    -- Como mucho un pomodoro abierto en todo el sistema. La regla vive en el
    -- índice para que no dependa de que el daemon se porte bien.
    --
    -- Se indexa la EXPRESIÓN, no la columna: SQLite considera distintos todos
    -- los NULL, así que un índice sobre `outcome` dejaría entrar mil filas
    -- abiertas. `outcome IS NULL` vale 1 en todas ellas, y ahí sí choca.
    CREATE UNIQUE INDEX pomodoros_solo_uno_abierto
        ON pomodoros((outcome IS NULL)) WHERE outcome IS NULL;
    CREATE INDEX pomodoros_por_tarea ON pomodoros(task_id);

    CREATE TABLE interruptions (
        id          INTEGER PRIMARY KEY,
        task_id     INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
        pomodoro_id INTEGER REFERENCES pomodoros(id) ON DELETE SET NULL,
        -- 'internal' es la comilla simple de Cirillo, 'external' la doble.
        kind        TEXT    NOT NULL CHECK (kind IN ('internal','external')),
        at          TEXT    NOT NULL,
        note        TEXT
    );

    CREATE INDEX interruptions_por_tarea ON interruptions(task_id);

    -- Un tramo de trabajo real sobre una tarea: desde que se arranca hasta
    -- que el ciclo termina, se aparca, se cambia de tarea o se da por hecha.
    -- Incluye el descanso que sigue al pomodoro —durante ese descanso la
    -- tarea sigue siendo la tuya— pero NO la noche entera si se olvidó
    -- aparcarla: el tramo se cierra solo cuando el reloj vuelve a cero.
    -- Sobrevive a los pomodoros anulados, que es lo que hace que el refri
    -- conserve el tiempo acumulado.
    CREATE TABLE task_sessions (
        id         INTEGER PRIMARY KEY,
        task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        started_at TEXT    NOT NULL,
        ended_at   TEXT,
        -- Por qué se cerró. Sin esto, "cuántas veces se aparcó la tarea" sería
        -- en realidad "cuántos ciclos tuvo", que no es lo mismo ni de lejos.
        end_reason TEXT    CHECK (end_reason IN
                          ('parked','switched','done','idle','recovered'))
    );

    CREATE UNIQUE INDEX task_sessions_una_abierta_por_tarea
        ON task_sessions(task_id) WHERE ended_at IS NULL;
    CREATE INDEX task_sessions_por_tarea ON task_sessions(task_id);

    -- El estado del reloj, para que un reinicio del daemon no pierda el
    -- pomodoro que estaba vivo. Una sola fila, siempre.
    CREATE TABLE timer (
        id                         INTEGER PRIMARY KEY CHECK (id = 1),
        state_json                 TEXT    NOT NULL,
        completed_since_long_break INTEGER NOT NULL DEFAULT 0,
        updated_at                 TEXT    NOT NULL
    );
    "#,
];

/// Sube la base hasta la última versión. Es idempotente.
pub fn migrate(conn: &Connection) -> Result<(), CoffeError> {
    let actual: i64 = conn.query_row("PRAGMA user_version", [], |f| f.get(0))?;
    let objetivo = MIGRACIONES.len() as i64;

    if actual > objetivo {
        return Err(CoffeError::BaseDelFuturo { encontrada: actual, soportada: objetivo });
    }

    for (i, sql) in MIGRACIONES.iter().enumerate().skip(actual as usize) {
        let version = i as i64 + 1;
        conn.execute_batch(sql)?;
        // `pragma_update` no admite parámetros ligados, y la versión sale de
        // un índice, no de entrada del usuario.
        conn.execute_batch(&format!("PRAGMA user_version = {version}"))?;
    }

    Ok(())
}
