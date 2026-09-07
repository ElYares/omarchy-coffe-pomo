//! El lado cliente del socket. Deliberadamente síncrono y sin dependencias:
//! `coffe bar` se ejecuta a cada rato y no puede pagar el arranque de un
//! runtime asíncrono para mandar una línea de JSON.

use crate::{Request, Response, Snapshot};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(
        "el daemon no está corriendo (socket {ruta}). Arráncalo con \
         `systemctl --user start coffe` o `coffe daemon`"
    )]
    SinDaemon { ruta: String },

    #[error("no se pudo hablar con el daemon: {0}")]
    Io(#[from] std::io::Error),

    #[error("el daemon contestó algo que no entiendo: {0}")]
    Protocolo(String),

    #[error("el daemon cerró la conexión sin contestar")]
    Colgado,

    /// El daemon entendió la petición y dijo que no.
    #[error("{0}")]
    Rechazado(String),
}

pub struct Client {
    stream: UnixStream,
    lector: BufReader<UnixStream>,
}

impl Client {
    pub fn connect(socket: &Path) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(socket).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                IpcError::SinDaemon { ruta: socket.display().to_string() }
            }
            _ => IpcError::Io(e),
        })?;
        let lector = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, lector })
    }

    /// Manda una petición y espera su respuesta.
    pub fn send(&mut self, req: &Request) -> Result<Snapshot, IpcError> {
        let linea = serde_json::to_string(req)
            .map_err(|e| IpcError::Protocolo(format!("no pude serializar: {e}")))?;
        writeln!(self.stream, "{linea}")?;
        self.stream.flush()?;

        match self.recibir()? {
            Response::Ok { snapshot } => Ok(*snapshot),
            Response::Error { message } => Err(IpcError::Rechazado(message)),
        }
    }

    /// Lee el siguiente empujón del daemon. Solo tiene sentido tras
    /// `Request::Subscribe`. Devuelve `None` cuando el daemon cierra.
    pub fn next_snapshot(&mut self) -> Result<Option<Snapshot>, IpcError> {
        match self.recibir() {
            Ok(Response::Ok { snapshot }) => Ok(Some(*snapshot)),
            Ok(Response::Error { message }) => Err(IpcError::Rechazado(message)),
            Err(IpcError::Colgado) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn recibir(&mut self) -> Result<Response, IpcError> {
        let mut linea = String::new();
        if self.lector.read_line(&mut linea)? == 0 {
            return Err(IpcError::Colgado);
        }
        serde_json::from_str(linea.trim())
            .map_err(|e| IpcError::Protocolo(format!("{e}: {}", linea.trim())))
    }
}

/// Conectar, preguntar una cosa y colgar. Es el caso de casi toda la CLI.
pub fn ask(socket: &Path, req: &Request) -> Result<Snapshot, IpcError> {
    Client::connect(socket)?.send(req)
}
