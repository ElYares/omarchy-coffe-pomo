//! Traer el backlog del vault de Obsidian al tablero.
//!
//! El vault manda sobre QUÉ hay que hacer; coffe manda sobre el tiempo. Por eso
//! esto solo lee: no escribe una línea en el vault ni intenta sincronizar en dos
//! direcciones, que es donde estas cosas se rompen.

use anyhow::{Context, Result};
use chrono::Utc;
use coffe_core::Db;
use coffe_core::config::Config;
use coffe_core::db::projects::{Project, slugify};
use coffe_core::db::tasks::Importacion;
use coffe_core::vault::{leer_backlog, proyectos_con_backlog};
use std::path::{Path, PathBuf};

use crate::VaultCmd;
use crate::tareas::resolver_proyecto;

pub fn ejecutar(db: &Db, cfg: &Config, cmd: VaultCmd) -> Result<()> {
    let raiz = raiz_del_vault(cfg)?;

    match cmd {
        VaultCmd::Scan => escanear(db, cfg, &raiz),
        VaultCmd::Import { project, dry_run, incluir_terminadas } => {
            importar(db, cfg, &raiz, project.as_deref(), dry_run, incluir_terminadas)
        }
    }
}

fn raiz_del_vault(cfg: &Config) -> Result<PathBuf> {
    if cfg.vault.path.trim().is_empty() {
        anyhow::bail!(
            "no hay vault configurado. Pon su ruta en {}:\n\n  [vault]\n  path = \"~/develop/docs/mi-vault\"",
            coffe_core::paths::config().display()
        );
    }
    let expandida = expandir(&cfg.vault.path);
    if !expandida.is_dir() {
        anyhow::bail!("el vault {} no existe", expandida.display());
    }
    Ok(expandida)
}

/// `~/...` a mano: traerse una dependencia entera para un carácter no compensa.
fn expandir(ruta: &str) -> PathBuf {
    match ruta.strip_prefix("~/") {
        Some(resto) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(resto),
        None => PathBuf::from(ruta),
    }
}

// ------------------------------------------------------------------ scan

fn escanear(db: &Db, cfg: &Config, raiz: &Path) -> Result<()> {
    let carpetas = proyectos_con_backlog(raiz, &cfg.vault.projects_dir)
        .with_context(|| format!("no pude leer {}", raiz.display()))?;
    let proyectos = db.proyectos(false)?;

    let mut sin_ligar = 0;
    println!("{:<26} {:>4}  PROYECTO EN COFFE", "CARPETA DEL VAULT", "HU");

    for carpeta in &carpetas {
        let notas = leer_backlog(raiz, &cfg.vault.projects_dir, carpeta)?;
        let vivas = notas.iter().filter(|n| !n.nota.terminal).count();

        let ligado = db.proyecto_por_vault(carpeta)?;
        let destino = match &ligado {
            Some(p) => db.ruta_proyecto(p.id)?,
            None => {
                sin_ligar += 1;
                match sugerencia(&proyectos, carpeta) {
                    Some(p) => format!("— ¿{}? ", p.name),
                    None => "—".to_string(),
                }
            }
        };

        println!("{carpeta:<26} {vivas:>4}  {destino}");
    }

    if sin_ligar > 0 {
        println!(
            "\n{sin_ligar} carpeta(s) sin ligar. Se ligan una a una, porque los nombres\n\
             no siempre coinciden (`tl-mas-server` en el vault es `strapp/tl-mas` aquí):\n\n  \
             coffe project vault strapp/tl-mas tl-mas-server"
        );
    }
    Ok(())
}

/// Una pista, no una decisión: se propone el proyecto cuyo slug se parezca más,
/// pero ligar sigue siendo cosa de quien sabe qué es qué.
fn sugerencia<'a>(proyectos: &'a [Project], carpeta: &str) -> Option<&'a Project> {
    let objetivo = slugify(carpeta);
    proyectos.iter().find(|p| {
        p.slug == objetivo
            || objetivo.starts_with(&format!("{}-", p.slug))
            || objetivo.contains(&p.slug)
    })
}

// ---------------------------------------------------------------- import

fn importar(
    db: &Db,
    cfg: &Config,
    raiz: &Path,
    solo: Option<&str>,
    dry_run: bool,
    incluir_terminadas: bool,
) -> Result<()> {
    let ligados: Vec<Project> = match solo {
        Some(r) => vec![db.proyecto(resolver_proyecto(db, r)?)?],
        None => db.proyectos(false)?.into_iter().filter(|p| p.vault_path.is_some()).collect(),
    };

    if ligados.is_empty() {
        println!("Ningún proyecto está ligado al vault. Mira `coffe vault scan`.");
        return Ok(());
    }

    let mut total = Importacion::default();

    for p in &ligados {
        let Some(carpeta) = &p.vault_path else {
            println!("{} no está ligado a ninguna carpeta del vault.", db.ruta_proyecto(p.id)?);
            continue;
        };
        let notas = leer_backlog(raiz, &cfg.vault.projects_dir, carpeta)?;

        if dry_run {
            let (nuevas, viejas): (Vec<_>, Vec<_>) =
                notas.iter().filter(|n| incluir_terminadas || !n.nota.terminal).partition(|n| {
                    db.tarea_por_vault(p.id, &n.nota.id).map(|t| t.is_none()).unwrap_or(true)
                });
            println!(
                "{}  ({carpeta}): {} nueva(s), {} ya estaban",
                db.ruta_proyecto(p.id)?,
                nuevas.len(),
                viejas.len()
            );
            for n in nuevas.iter().take(5) {
                println!("    + {}", n.nota.titulo);
            }
            if nuevas.len() > 5 {
                println!("    + y {} más", nuevas.len() - 5);
            }
            continue;
        }

        let r = db.sincronizar_notas(p.id, &notas, incluir_terminadas, Utc::now())?;
        println!(
            "{}  ({carpeta}): {} nueva(s), {} actualizada(s), {} omitida(s)",
            db.ruta_proyecto(p.id)?,
            r.creadas,
            r.actualizadas,
            r.omitidas
        );
        acumular(&mut total, r);
    }

    if dry_run {
        println!("\nNada escrito. Quita `--dry-run` para traerlas.");
        return Ok(());
    }

    contar_rarezas(&total);
    Ok(())
}

fn acumular(total: &mut Importacion, r: Importacion) {
    total.creadas += r.creadas;
    total.actualizadas += r.actualizadas;
    total.omitidas += r.omitidas;
    total.sin_prioridad.extend(r.sin_prioridad);
    total.estados_raros.extend(r.estados_raros);
    total.prioridad_congelada.extend(r.prioridad_congelada);
}

/// Lo que el importador no supo interpretar se dice en voz alta. Tragárselo en
/// silencio es como acaban las importaciones perdiendo trabajo sin que nadie se
/// entere hasta meses después.
fn contar_rarezas(t: &Importacion) {
    if !t.estados_raros.is_empty() {
        println!("\nEstados que no reconozco (entraron como pendientes):");
        for (titulo, estado) in &t.estados_raros {
            println!("  {titulo}  →  {estado:?}");
        }
    }
    if !t.sin_prioridad.is_empty() {
        println!(
            "\n{} nota(s) sin `prioridad` en el frontmatter; entraron como media.",
            t.sin_prioridad.len()
        );
    }
    if !t.prioridad_congelada.is_empty() {
        println!("\nEstas ya se trabajaron, así que su prioridad no se tocó:");
        for titulo in &t.prioridad_congelada {
            println!("  {titulo}");
        }
    }
}
