//! El daemon: el único dueño del reloj.
//!
//! Escucha en un socket unix, aplica los comandos que le llegan y empuja el
//! estado a quien esté suscrito. Todo lo que sabe del pomodoro se lo pregunta a
//! `coffe_core::Service`; aquí solo vive lo que tiene que ver con el sistema:
//! el socket, el temporizador, las notificaciones y la señal a waybar.

use anyhow::{Context, Result};
use chrono::Utc;
use coffe_core::machine::Command;
use coffe_core::service::{self, Service};
use coffe_core::{Config, Db, paths};
use coffe_ipc::{Request, Response, Snapshot};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, broadcast};

use crate::vista;

/// La señal en la que escucha el módulo de waybar. Las 7 a la 14 ya están
/// tomadas por otros módulos de la barra.
const SENAL_WAYBAR: &str = "-RTMIN+15";

/// Cada cuánto se refresca la cuenta atrás de los suscritos.
const LATIDO: std::time::Duration = std::time::Duration::from_secs(1);

pub struct Daemon {
    estado: Arc<Mutex<Estado>>,
    tx: broadcast::Sender<Snapshot>,
    socket: PathBuf,
}

struct Estado {
    svc: Service,
    /// Las partes del snapshot que cuestan una consulta. Se recalculan cuando
    /// algo cambia, no cada segundo: la cuenta atrás no necesita volver a
    /// preguntar cuántas tazas hay en el bote.
    base: vista::Base,
}

impl Estado {
    fn snapshot(&self) -> Snapshot {
        vista::snapshot(&self.svc, &self.base, Utc::now())
    }

    fn refrescar(&mut self) -> Result<()> {
        self.base = vista::Base::leer(&self.svc, Utc::now())?;
        Ok(())
    }
}

impl Daemon {
    pub fn nuevo(cfg: Config) -> Result<Self> {
        let socket = paths::socket();
        comprobar_socket(&socket)?;

        let db = Db::open(&paths::database()).context("no pude abrir la base")?;
        let (svc, recuperados) = Service::arrancar(db, cfg, Utc::now())?;

        if service::hubo_recuperacion(&recuperados) {
            eprintln!("coffe: había trabajo a medias de una sesión anterior; lo cerré");
        }
        for efecto in &recuperados {
            if let Some((titulo, cuerpo)) = service::aviso(efecto) {
                notificar(titulo, &cuerpo);
            }
        }

        let base = vista::Base::leer(&svc, Utc::now())?;
        let (tx, _) = broadcast::channel(64);

        Ok(Self { estado: Arc::new(Mutex::new(Estado { svc, base })), tx, socket })
    }

    pub async fn correr(self) -> Result<()> {
        if let Some(dir) = self.socket.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let escucha = UnixListener::bind(&self.socket)
            .with_context(|| format!("no pude abrir el socket {}", self.socket.display()))?;
        eprintln!("coffe: escuchando en {}", self.socket.display());

        let reloj = tokio::spawn(latir(self.estado.clone(), self.tx.clone()));

        let salida = tokio::select! {
            r = aceptar(escucha, self.estado.clone(), self.tx.clone()) => r,
            _ = apagado() => {
                eprintln!("coffe: cerrando");
                Ok(())
            }
        };

        reloj.abort();
        // El socket es un archivo: si se queda, el próximo arranque se cree que
        // ya hay un daemon vivo.
        let _ = std::fs::remove_file(&self.socket);
        salida
    }
}

/// El reloj. Duerme hasta lo que ocurra antes: el próximo vencimiento o el
/// siguiente latido. Con el reloj parado no hace nada de nada.
async fn latir(estado: Arc<Mutex<Estado>>, tx: broadcast::Sender<Snapshot>) {
    loop {
        let espera = {
            let est = estado.lock().await;
            match est.svc.proximo_vencimiento() {
                None => LATIDO,
                Some(fin) => {
                    let falta = (fin - Utc::now()).to_std().unwrap_or_default();
                    falta.min(LATIDO)
                }
            }
        };
        tokio::time::sleep(espera).await;

        let mut est = estado.lock().await;
        if est.svc.state().is_idle() {
            continue;
        }

        let vencido = est.svc.proximo_vencimiento().is_some_and(|fin| Utc::now() >= fin);
        if vencido {
            match est.svc.ejecutar(Command::Tick, Utc::now()) {
                Ok(fx) => {
                    if let Err(e) = est.refrescar() {
                        eprintln!("coffe: no pude refrescar el estado: {e}");
                    }
                    let snap = est.snapshot();
                    drop(est);
                    anunciar(&fx, &snap, &tx);
                    continue;
                }
                Err(e) => eprintln!("coffe: el reloj falló: {e}"),
            }
        }

        // Latido normal: solo la cuenta atrás, sin tocar la base.
        let _ = tx.send(est.snapshot());
    }
}

async fn aceptar(
    escucha: UnixListener,
    estado: Arc<Mutex<Estado>>,
    tx: broadcast::Sender<Snapshot>,
) -> Result<()> {
    loop {
        let (flujo, _) = escucha.accept().await?;
        let estado = estado.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = atender(flujo, estado, tx).await {
                eprintln!("coffe: conexión terminada: {e}");
            }
        });
    }
}

async fn atender(
    flujo: UnixStream,
    estado: Arc<Mutex<Estado>>,
    tx: broadcast::Sender<Snapshot>,
) -> Result<()> {
    let (lectura, mut escritura) = flujo.into_split();
    let mut lineas = BufReader::new(lectura).lines();

    while let Some(linea) = lineas.next_line().await? {
        if linea.trim().is_empty() {
            continue;
        }

        let peticion: Request = match serde_json::from_str(linea.trim()) {
            Ok(p) => p,
            Err(e) => {
                responder(&mut escritura, &Response::Error { message: format!("{e}") }).await?;
                continue;
            }
        };

        if matches!(peticion, Request::Subscribe) {
            let inicial = estado.lock().await.snapshot();
            responder(&mut escritura, &Response::Ok { snapshot: Box::new(inicial) }).await?;

            let mut rx = tx.subscribe();
            while let Ok(snap) = rx.recv().await {
                if responder(&mut escritura, &Response::Ok { snapshot: Box::new(snap) })
                    .await
                    .is_err()
                {
                    // El cliente se fue; no es un error que reportar.
                    return Ok(());
                }
            }
            return Ok(());
        }

        let respuesta = ejecutar(&peticion, &estado, &tx).await;
        responder(&mut escritura, &respuesta).await?;
    }
    Ok(())
}

async fn ejecutar(
    peticion: &Request,
    estado: &Arc<Mutex<Estado>>,
    tx: &broadcast::Sender<Snapshot>,
) -> Response {
    let now = Utc::now();
    let cmd = match peticion {
        Request::Status => None,
        Request::Start { task_id } => Some(Command::Start { task_id: *task_id }),
        Request::Pause => Some(Command::Pause),
        Request::Void { reason } => Some(Command::Void { reason: *reason }),
        Request::Switch { task_id } => Some(Command::Switch { task_id: *task_id }),
        Request::Done => Some(Command::Done),
        Request::Interrupt { kind, .. } => Some(Command::Interrupt { kind: *kind }),
        Request::SkipBreak => Some(Command::SkipBreak),
        Request::Subscribe => unreachable!("se atiende antes"),
    };

    let mut est = estado.lock().await;

    let fx = match cmd {
        // `Status` también adelanta el reloj: preguntar la hora nunca debe
        // devolver un estado que ya venció.
        None => match est.svc.ejecutar(Command::Tick, now) {
            Ok(fx) => fx,
            Err(e) => return Response::Error { message: e.to_string() },
        },
        Some(c) => match est.svc.ejecutar(c, now) {
            Ok(fx) => fx,
            Err(e) => return Response::Error { message: e.to_string() },
        },
    };

    if let Err(e) = est.refrescar() {
        return Response::Error { message: e.to_string() };
    }
    let snap = est.snapshot();
    drop(est);

    anunciar(&fx, &snap, tx);
    Response::Ok { snapshot: Box::new(snap) }
}

/// Avisa a todo el mundo de lo que pasó: suscritos, escritorio y barra.
fn anunciar(fx: &[coffe_core::Effect], snap: &Snapshot, tx: &broadcast::Sender<Snapshot>) {
    let _ = tx.send(snap.clone());

    for efecto in fx {
        if let Some((titulo, cuerpo)) = service::aviso(efecto) {
            notificar(titulo, &cuerpo);
        }
    }

    if fx.iter().any(service::cambia_la_barra) {
        senalar_waybar();
    }
}

async fn responder(
    escritura: &mut tokio::net::unix::OwnedWriteHalf,
    respuesta: &Response,
) -> Result<()> {
    let mut linea = serde_json::to_string(respuesta)?;
    linea.push('\n');
    escritura.write_all(linea.as_bytes()).await?;
    escritura.flush().await?;
    Ok(())
}

async fn apagado() {
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => return std::future::pending().await,
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

/// Si el socket existe pero nadie contesta, es de un daemon muerto y se puede
/// tirar. Si contesta, hay otro corriendo y este sobra.
fn comprobar_socket(socket: &Path) -> Result<()> {
    if !socket.exists() {
        return Ok(());
    }
    if std::os::unix::net::UnixStream::connect(socket).is_ok() {
        anyhow::bail!("ya hay un daemon escuchando en {}", socket.display());
    }
    std::fs::remove_file(socket)
        .with_context(|| format!("no pude quitar el socket muerto {}", socket.display()))?;
    Ok(())
}

fn notificar(titulo: &str, cuerpo: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "coffe", "-i", "coffee", titulo, cuerpo])
        .spawn();
}

fn senalar_waybar() {
    let _ = std::process::Command::new("pkill").args([SENAL_WAYBAR, "waybar"]).spawn();
}
