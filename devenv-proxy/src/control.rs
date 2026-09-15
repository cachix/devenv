use crate::Route;
#[cfg(feature = "server")]
use crate::RouteTable;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[cfg(feature = "server")]
use std::{
    fs,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::UnixListener,
    },
    thread::{self, JoinHandle},
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    Register { route: Route },
    Unregister { hostname: String, owner: String },
    ReplaceOwner { owner: String, routes: Vec<Route> },
    List,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ControlResponse {
    Info {
        pid: u32,
        https_listen: Option<std::net::SocketAddr>,
    },
    Ok {
        routes: Option<Vec<Route>>,
        removed: Option<bool>,
    },
    Error {
        message: String,
    },
}

impl ControlResponse {
    pub fn into_result(self) -> Result<Self> {
        match self {
            Self::Error { message } => bail!(message),
            response => Ok(response),
        }
    }
}

pub fn request(socket: &Path, request: &ControlRequest) -> Result<ControlResponse> {
    let mut stream = UnixStream::connect(socket).with_context(|| {
        format!(
            "failed to connect to proxy control socket {}",
            socket.display()
        )
    })?;
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .context("failed to set proxy response timeout")?;
    stream
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .context("failed to set proxy request timeout")?;
    serde_json::to_writer(&mut stream, request).context("failed to encode proxy request")?;
    stream
        .write_all(b"\n")
        .context("failed to send proxy request")?;

    let mut response = String::new();
    BufReader::new(stream)
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut response)
        .context("failed to read proxy response")?;
    serde_json::from_str(&response).context("failed to decode proxy response")
}

#[cfg(feature = "server")]
pub fn serve_control(socket: &Path, routes: RouteTable) -> Result<JoinHandle<()>> {
    prepare_socket(socket)?;
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("failed to bind proxy control socket {}", socket.display()))?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))
        .context("failed to secure proxy control socket")?;

    thread::Builder::new()
        .name("devenv-proxy-control".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => handle_connection(stream, &routes),
                    Err(error) => eprintln!("proxy control connection failed: {error}"),
                }
            }
        })
        .context("failed to start proxy control thread")
}

#[cfg(feature = "server")]
fn prepare_socket(socket: &Path) -> Result<()> {
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let Ok(metadata) = fs::symlink_metadata(socket) else {
        return Ok(());
    };
    if !metadata.file_type().is_socket() {
        bail!("refusing to replace non-socket path {}", socket.display());
    }
    if UnixStream::connect(socket).is_ok() {
        bail!("a proxy is already listening on {}", socket.display());
    }
    fs::remove_file(socket)
        .with_context(|| format!("failed to remove stale socket {}", socket.display()))?;
    Ok(())
}

#[cfg(feature = "server")]
fn handle_connection(mut stream: UnixStream, routes: &RouteTable) {
    if let Err(error) = stream.set_read_timeout(Some(CONTROL_TIMEOUT)) {
        eprintln!("failed to set proxy control timeout: {error}");
        return;
    }
    let response = parse_request(&stream).and_then(|request| dispatch(request, routes));
    let response = match response {
        Ok(response) => response,
        Err(error) => ControlResponse::Error {
            message: format!("{error:#}"),
        },
    };
    let write_result = serde_json::to_writer(&mut stream, &response)
        .context("failed to encode proxy control response")
        .and_then(|()| {
            stream
                .write_all(b"\n")
                .context("failed to terminate proxy control response")
        });
    if let Err(error) = write_result {
        eprintln!("failed to write proxy control response: {error}");
    }
}

#[cfg(feature = "server")]
fn parse_request(stream: &UnixStream) -> Result<ControlRequest> {
    let mut line = String::new();
    BufReader::new(stream)
        .take(MAX_REQUEST_BYTES)
        .read_line(&mut line)
        .context("failed to read proxy request")?;
    if line.is_empty() {
        bail!("empty proxy request");
    }
    serde_json::from_str(&line).context("invalid proxy request")
}

#[cfg(feature = "server")]
fn dispatch(request: ControlRequest, routes: &RouteTable) -> Result<ControlResponse> {
    match request {
        ControlRequest::Status => Ok(ControlResponse::Info {
            pid: std::process::id(),
            https_listen: routes.https_listen,
        }),
        ControlRequest::Register { route } => {
            routes.register(route)?;
            Ok(ControlResponse::Ok {
                routes: None,
                removed: None,
            })
        }
        ControlRequest::Unregister { hostname, owner } => {
            let removed = routes.unregister(&hostname, &owner)?;
            Ok(ControlResponse::Ok {
                routes: None,
                removed: Some(removed),
            })
        }
        ControlRequest::ReplaceOwner {
            owner,
            routes: replacement,
        } => {
            routes.replace_owner(&owner, replacement)?;
            Ok(ControlResponse::Ok {
                routes: None,
                removed: None,
            })
        }
        ControlRequest::List => Ok(ControlResponse::Ok {
            routes: Some(routes.list()),
            removed: None,
        }),
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[test]
    fn serves_requests_over_a_private_socket() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("proxy.sock");
        let _server = serve_control(&socket, RouteTable::default()).unwrap();
        let route = Route {
            hostname: "web.demo.localhost".to_owned(),
            upstream: SocketAddr::from(([127, 0, 0, 1], 3000)),
            owner: "demo".to_owned(),
            tls: None,
        };

        request(
            &socket,
            &ControlRequest::Register {
                route: route.clone(),
            },
        )
        .unwrap()
        .into_result()
        .unwrap();
        let response = request(&socket, &ControlRequest::List)
            .unwrap()
            .into_result()
            .unwrap();
        match response {
            ControlResponse::Ok {
                routes: Some(routes),
                ..
            } => assert_eq!(routes, vec![route]),
            _ => panic!("unexpected response"),
        }
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn never_replaces_a_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("proxy.sock");
        fs::write(&socket, "keep me").unwrap();
        assert!(serve_control(&socket, RouteTable::default()).is_err());
        assert_eq!(fs::read_to_string(socket).unwrap(), "keep me");
    }
}
