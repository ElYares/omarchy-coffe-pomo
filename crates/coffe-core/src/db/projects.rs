//! El árbol de proyectos.

use super::{Db, a_texto, de_texto};
use crate::error::CoffeError;
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub slug: String,
    pub repo_path: Option<String>,
    pub vault_path: Option<String>,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NuevoProyecto {
    pub parent_id: Option<i64>,
    pub name: String,
    pub repo_path: Option<String>,
    pub vault_path: Option<String>,
}

impl Db {
    pub fn crear_proyecto(
        &self,
        nuevo: NuevoProyecto,
        now: DateTime<Utc>,
    ) -> Result<i64, CoffeError> {
        let slug = slugify(&nuevo.name);
        self.conn.execute(
            "INSERT INTO projects (parent_id, name, slug, repo_path, vault_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                nuevo.parent_id,
                nuevo.name,
                slug,
                nuevo.repo_path,
                nuevo.vault_path,
                a_texto(now)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn proyecto(&self, id: i64) -> Result<Project, CoffeError> {
        self.conn
            .query_row("SELECT * FROM projects WHERE id = ?1", params![id], fila_a_proyecto)
            .optional()?
            .ok_or(CoffeError::NoExiste { que: "proyecto", id })?
    }

    /// Todos los proyectos, ordenados de forma que un padre siempre sale antes
    /// que sus hijos: así la interfaz puede pintar el árbol de una pasada.
    pub fn proyectos(&self, incluir_archivados: bool) -> Result<Vec<Project>, CoffeError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE arbol(id, nivel, ruta) AS (
                 SELECT id, 0, slug FROM projects WHERE parent_id IS NULL
                 UNION ALL
                 SELECT p.id, a.nivel + 1, a.ruta || '/' || p.slug
                   FROM projects p JOIN arbol a ON p.parent_id = a.id
             )
             SELECT p.* FROM projects p JOIN arbol a ON a.id = p.id
             WHERE (?1 OR p.archived = 0)
             ORDER BY a.ruta",
        )?;
        let filas = stmt.query_map(params![incluir_archivados], fila_a_proyecto)?;
        filas.collect::<Result<Result<Vec<_>, _>, _>>()?
    }

    /// La ruta legible del proyecto: `clientes / nutricore`.
    pub fn ruta_proyecto(&self, id: i64) -> Result<String, CoffeError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE subida(id, name, parent_id, nivel) AS (
                 SELECT id, name, parent_id, 0 FROM projects WHERE id = ?1
                 UNION ALL
                 SELECT p.id, p.name, p.parent_id, s.nivel + 1
                   FROM projects p JOIN subida s ON p.id = s.parent_id
             )
             SELECT name FROM subida ORDER BY nivel DESC",
        )?;
        let nombres: Vec<String> =
            stmt.query_map(params![id], |f| f.get(0))?.collect::<Result<_, _>>()?;

        if nombres.is_empty() {
            return Err(CoffeError::NoExiste { que: "proyecto", id });
        }
        Ok(nombres.join(" / "))
    }

    /// El proyecto cuyo `repo_path` contiene esta ruta. Se queda con el más
    /// específico: un monorepo padre no debe ganarle al subproyecto.
    pub fn proyecto_por_ruta(&self, cwd: &str) -> Result<Option<Project>, CoffeError> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM projects
             WHERE repo_path IS NOT NULL
               AND archived = 0
               AND (?1 = repo_path OR ?1 LIKE repo_path || '/%')
             ORDER BY length(repo_path) DESC
             LIMIT 1",
        )?;
        stmt.query_row(params![cwd], fila_a_proyecto).optional()?.transpose()
    }
}

fn fila_a_proyecto(f: &Row<'_>) -> rusqlite::Result<Result<Project, CoffeError>> {
    let created_at: String = f.get("created_at")?;
    Ok((|| {
        Ok(Project {
            id: f.get("id")?,
            parent_id: f.get("parent_id")?,
            name: f.get("name")?,
            slug: f.get("slug")?,
            repo_path: f.get("repo_path")?,
            vault_path: f.get("vault_path")?,
            archived: f.get::<_, i64>("archived")? != 0,
            created_at: de_texto(&created_at)?,
        })
    })())
}

/// `Luz Gutiérrez` -> `luz-gutierrez`. Sin dependencias: solo hace falta que
/// sea estable y legible en una ruta.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut guion_pendiente = false;

    for c in s.trim().chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
            'ñ' | 'Ñ' => 'n',
            otro => otro,
        };

        if c.is_ascii_alphanumeric() {
            if guion_pendiente && !out.is_empty() {
                out.push('-');
            }
            guion_pendiente = false;
            out.push(c.to_ascii_lowercase());
        } else {
            guion_pendiente = true;
        }
    }

    out
}

/// Administración del árbol. Lo que hace falta para que un proyecto no sea
/// algo que se crea una vez y ya no se puede tocar.
impl Db {
    pub fn renombrar_proyecto(&self, id: i64, nombre: &str) -> Result<(), CoffeError> {
        let n = self.conn.execute(
            "UPDATE projects SET name = ?2, slug = ?3 WHERE id = ?1",
            params![id, nombre, slugify(nombre)],
        )?;
        if n == 0 {
            return Err(CoffeError::NoExiste { que: "proyecto", id });
        }
        Ok(())
    }

    /// Lo cuelga de otro padre, o de la raíz con `None`.
    pub fn mover_proyecto(&self, id: i64, nuevo_padre: Option<i64>) -> Result<(), CoffeError> {
        self.proyecto(id)?;

        if let Some(destino) = nuevo_padre {
            self.proyecto(destino)?;
            // Colgar un proyecto de su propio descendiente dejaria el subarbol
            // suelto: ni sale del arbol ni se puede volver a alcanzar. La base
            // lo aceptaria tan contenta, asi que la regla vive aqui.
            if destino == id || self.descendientes(id)?.contains(&destino) {
                return Err(CoffeError::CicloEnElArbol { id, destino });
            }
        }

        self.conn.execute(
            "UPDATE projects SET parent_id = ?2 WHERE id = ?1",
            params![id, nuevo_padre],
        )?;
        Ok(())
    }

    /// La carpeta del repo, o `None` para quitarla.
    pub fn fijar_repo(&self, id: i64, repo: Option<&str>) -> Result<(), CoffeError> {
        let n = self
            .conn
            .execute("UPDATE projects SET repo_path = ?2 WHERE id = ?1", params![id, repo])?;
        if n == 0 {
            return Err(CoffeError::NoExiste { que: "proyecto", id });
        }
        Ok(())
    }

    /// Todos los ids que cuelgan de este, a cualquier profundidad.
    pub fn descendientes(&self, id: i64) -> Result<Vec<i64>, CoffeError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE bajada(id) AS (
                 SELECT id FROM projects WHERE parent_id = ?1
                 UNION ALL
                 SELECT p.id FROM projects p JOIN bajada b ON p.parent_id = b.id
             )
             SELECT id FROM bajada",
        )?;
        let filas = stmt.query_map(params![id], |f| f.get(0))?;
        Ok(filas.collect::<Result<_, _>>()?)
    }

    /// Cuántos subproyectos y cuántas tareas cuelgan de él. Es lo que se mira
    /// antes de dejar borrar.
    pub fn contenido_proyecto(&self, id: i64) -> Result<(u32, u32), CoffeError> {
        let hijos = self.descendientes(id)?;
        let mut ids = hijos.clone();
        ids.push(id);

        let marcas = vec!["?"; ids.len()].join(",");
        let sql = format!("SELECT COUNT(*) FROM tasks WHERE project_id IN ({marcas})");
        let refs: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
        let tareas: u32 = self.conn.query_row(&sql, refs.as_slice(), |f| f.get(0))?;

        Ok((hijos.len() as u32, tareas))
    }

    /// Archiva el proyecto **y todo lo que cuelgue de él**: archivar un padre
    /// dejando visibles a los hijos sería un árbol con agujeros.
    pub fn archivar_proyecto(&self, id: i64, archivado: bool) -> Result<u32, CoffeError> {
        self.proyecto(id)?;
        let mut ids = self.descendientes(id)?;
        ids.push(id);

        let marcas = vec!["?"; ids.len()].join(",");
        let sql = format!("UPDATE projects SET archived = ?1 WHERE id IN ({marcas})");
        let mut vals: Vec<&dyn rusqlite::ToSql> = vec![&archivado];
        vals.extend(ids.iter().map(|i| i as &dyn rusqlite::ToSql));

        Ok(self.conn.execute(&sql, vals.as_slice())? as u32)
    }

    /// Borra de verdad. Sin `force` se niega si hay algo dentro, porque el
    /// esquema borra en cascada: un `rm` distraído se lleva el historial de
    /// tiempo de todas las tareas que colgaran del proyecto.
    pub fn borrar_proyecto(&self, id: i64, force: bool) -> Result<(), CoffeError> {
        self.proyecto(id)?;
        let (hijos, tareas) = self.contenido_proyecto(id)?;

        if !force && (hijos > 0 || tareas > 0) {
            return Err(CoffeError::ProyectoConContenido { id, hijos, tareas });
        }
        self.conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }
}
