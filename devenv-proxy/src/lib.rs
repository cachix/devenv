//! Shared HTTP reverse proxy for friendly devenv `.localhost` URLs.

mod control;
mod routes;

#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
pub use control::{ControlRequest, ControlResponse, request, serve_control};
use pingora_core::{
    listeners::ConnectionFilter,
    server::{Server, configuration::ServerConf},
    upstreams::peer::HttpPeer,
};
use pingora_proxy::{ProxyHttp, Session, http_proxy_service};
pub use routes::{Route, RouteTable, normalize_hostname};
use std::{
    env,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
#[cfg(target_os = "macos")]
use std::{num::NonZeroU32, os::fd::IntoRawFd};

const NOT_FOUND: &[u8] = b"No devenv process is registered for this hostname.\n";
pub const HEALTH_HOSTNAME: &str = "_devenv-proxy.localhost";

struct Router {
    routes: RouteTable,
}

struct Upstream {
    address: SocketAddr,
    fallback: Option<SocketAddr>,
}

impl Upstream {
    fn new(address: SocketAddr) -> Self {
        // Development servers binding to localhost may listen on either family.
        // Preserve explicitly selected addresses elsewhere in the loopback range.
        let fallback = match address.ip() {
            IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST => Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            IpAddr::V6(ip) if ip == Ipv6Addr::LOCALHOST => Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            _ => None,
        }
        .map(|ip| SocketAddr::new(ip, address.port()));
        Self { address, fallback }
    }
}

/// The macOS low-port listener uses a wildcard bind, so reject non-loopback
/// peers immediately after accept and before any HTTP parsing.
#[derive(Debug)]
struct LoopbackOnly;

#[async_trait]
impl ConnectionFilter for LoopbackOnly {
    async fn should_accept(&self, address: Option<&SocketAddr>) -> bool {
        is_loopback_peer(address)
    }
}

fn is_loopback_peer(address: Option<&SocketAddr>) -> bool {
    address.is_some_and(|address| address.ip().is_loopback())
}

#[async_trait]
impl ProxyHttp for Router {
    type CTX = Option<Upstream>;

    fn new_ctx(&self) -> Self::CTX {
        None
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<bool> {
        let host = request_host(session);
        if host.as_deref() == Some(HEALTH_HOSTNAME) {
            let _ = session.respond_error_with_body(204, Bytes::new()).await;
            return Ok(true);
        }
        *ctx = host
            .as_deref()
            .and_then(|host| self.routes.resolve(host))
            .map(Upstream::new);

        if ctx.is_none() {
            let _ = session
                .respond_error_with_body(404, Bytes::from_static(NOT_FOUND))
                .await;
            return Ok(true);
        }

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<Box<HttpPeer>> {
        // `request_filter` populates the context or completes the response early.
        let upstream = ctx
            .as_ref()
            .expect("routed requests always have an upstream");
        Ok(Box::new(HttpPeer::new(
            upstream.address,
            false,
            String::new(),
        )))
    }

    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        mut error: Box<pingora_core::Error>,
    ) -> Box<pingora_core::Error> {
        // Retry only connection establishment, before any request is sent, and
        // try the other loopback family at most once per request.
        let fallback = ctx.as_mut().is_some_and(|upstream| {
            if let Some(address) = upstream.fallback.take() {
                upstream.address = address;
                true
            } else {
                false
            }
        });
        error.set_retry(fallback);
        error
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut pingora_http::RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()> {
        if let Some(host) = request_host(session) {
            upstream_request.insert_header("x-forwarded-host", host)?;
        }
        upstream_request.insert_header("x-forwarded-proto", "http")?;
        if let Some(client) = session
            .as_downstream()
            .client_addr()
            .and_then(|address| address.as_inet())
        {
            upstream_request.insert_header("x-forwarded-for", client.ip().to_string())?;
        }
        Ok(())
    }
}

fn request_host(session: &Session) -> Option<String> {
    if let Some(authority) = session.req_header().uri.authority() {
        return Some(authority.host().to_owned());
    }

    let host = session.req_header().headers.get("host")?.to_str().ok()?;
    Some(strip_port(host).to_owned())
}

fn strip_port(host: &str) -> &str {
    // Bracketed IPv6 is not a valid devenv hostname, but avoid mangling it here.
    if host.starts_with('[') {
        return host;
    }
    host.rsplit_once(':')
        .filter(|(_, port)| port.parse::<u16>().is_ok())
        .map_or(host, |(hostname, _)| hostname)
}

/// Resolve the per-user control socket shared by the daemon and CLI.
pub fn default_control_socket() -> PathBuf {
    env::var_os("DEVENV_PROXY_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_RUNTIME_DIR").map_or_else(
                || {
                    env::temp_dir().join(format!(
                        "devenv-proxy-{}.sock",
                        whoami::username()
                            .unwrap_or_else(|_| "unknown".to_owned())
                            .replace(['/', '\\'], "-")
                    ))
                },
                |runtime_dir| PathBuf::from(runtime_dir).join("devenv/proxy.sock"),
            )
        })
}

/// Run the proxy in the foreground.
///
/// The control listener is deliberately separate from Pingora's data plane. It
/// is local-only and uses a mode-0600 Unix socket.
#[cfg(unix)]
pub fn run(listen: SocketAddr, control_socket: &Path) -> Result<()> {
    let routes = RouteTable::default();
    let _control = serve_control(control_socket, routes.clone())?;

    // Pingora defaults to a five-minute drain on SIGTERM. Keep local daemon
    // restarts short, including when development servers hold WebSockets open.
    let mut server = Server::new_with_opt_and_conf(
        None,
        ServerConf {
            grace_period_seconds: Some(1),
            graceful_shutdown_timeout_seconds: Some(1),
            ..Default::default()
        },
    );
    server.bootstrap();

    let mut service = http_proxy_service(&server.configuration, Router { routes });
    service.set_connection_filter(Arc::new(LoopbackOnly));

    #[cfg(target_os = "macos")]
    if let Some(prebound) = prebind_macos_low_port(listen)? {
        service.add_tcp(&prebound.bind);
        server.add_service(PreboundService {
            inner: service,
            prebound: Some(prebound),
        });
    } else {
        service.add_tcp(&listen.to_string());
        server.add_service(service);
    }

    #[cfg(not(target_os = "macos"))]
    {
        service.add_tcp(&listen.to_string());
        server.add_service(service);
    }
    server.run_forever();
}

#[cfg(target_os = "macos")]
struct PreboundListener {
    bind: String,
    socket: socket2::Socket,
}

/// Bind a low port without privilege by using Darwin's wildcard exception,
/// while `IP_BOUND_IF`/`IPV6_BOUND_IF` restricts the socket to `lo0` in the
/// kernel. Pingora adopts this descriptor through its inherited-FD table.
#[cfg(target_os = "macos")]
fn prebind_macos_low_port(listen: SocketAddr) -> Result<Option<PreboundListener>> {
    if listen.port() >= 1024 || !listen.ip().is_loopback() {
        return Ok(None);
    }

    let (domain, wildcard) = match listen.ip() {
        IpAddr::V4(_) => (
            socket2::Domain::IPV4,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), listen.port()),
        ),
        IpAddr::V6(_) => (
            socket2::Domain::IPV6,
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), listen.port()),
        ),
    };
    let socket = socket2::Socket::new(domain, socket2::Type::STREAM, Some(socket2::Protocol::TCP))
        .context("failed to create the macOS proxy listener")?;
    socket
        .set_reuse_address(true)
        .context("failed to configure the macOS proxy listener")?;

    // SAFETY: `lo0` is a static, null-terminated interface name.
    let interface = unsafe { libc::if_nametoindex(c"lo0".as_ptr()) };
    let interface =
        NonZeroU32::new(interface).context("macOS loopback interface lo0 is missing")?;
    match listen.ip() {
        IpAddr::V4(_) => socket
            .bind_device_by_index_v4(Some(interface))
            .context("failed to restrict the proxy listener to macOS lo0")?,
        IpAddr::V6(_) => socket
            .bind_device_by_index_v6(Some(interface))
            .context("failed to restrict the proxy listener to macOS lo0")?,
    }
    socket.bind(&wildcard.into()).with_context(|| {
        format!(
            "failed to bind macOS loopback proxy on port {}",
            listen.port()
        )
    })?;
    socket
        .set_nonblocking(true)
        .context("failed to make the macOS proxy listener nonblocking")?;

    Ok(Some(PreboundListener {
        bind: wildcard.to_string(),
        socket,
    }))
}

#[cfg(target_os = "macos")]
struct PreboundService<A> {
    inner: pingora_core::services::listening::Service<A>,
    prebound: Option<PreboundListener>,
}

#[cfg(target_os = "macos")]
#[async_trait]
impl<A> pingora_core::services::ServiceWithDependents for PreboundService<A>
where
    A: pingora_core::apps::ServerApp + Send + Sync + 'static,
{
    async fn start_service(
        &mut self,
        fds: Option<pingora_core::server::ListenFds>,
        shutdown: pingora_core::server::ShutdownWatch,
        listeners_per_fd: usize,
        ready: pingora_core::services::ServiceReadyNotifier,
    ) {
        let fds = fds.expect("Pingora provides an inherited listener table on Unix");
        if let Some(prebound) = self.prebound.take() {
            fds.lock()
                .await
                .add(prebound.bind, prebound.socket.into_raw_fd());
        }
        ready.notify_ready();
        pingora_core::services::Service::start_service(
            &mut self.inner,
            Some(fds),
            shutdown,
            listeners_per_fd,
        )
        .await;
    }

    fn name(&self) -> &str {
        pingora_core::services::Service::name(&self.inner)
    }

    fn threads(&self) -> Option<usize> {
        pingora_core::services::Service::threads(&self.inner)
    }
}

#[cfg(not(unix))]
pub fn run(_listen: SocketAddr, _control_socket: &Path) -> Result<()> {
    anyhow::bail!("devenv proxy currently requires Unix domain sockets")
}

#[cfg(test)]
mod tests {
    use super::{is_loopback_peer, strip_port};
    use std::net::SocketAddr;

    #[test]
    fn strips_a_numeric_port() {
        assert_eq!(strip_port("web.demo.localhost:8080"), "web.demo.localhost");
        assert_eq!(strip_port("web.demo.localhost"), "web.demo.localhost");
        assert_eq!(
            strip_port("web.demo.localhost:nope"),
            "web.demo.localhost:nope"
        );
    }

    #[test]
    fn accepts_only_loopback_peers() {
        let ipv4: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let ipv6: SocketAddr = "[::1]:1234".parse().unwrap();
        let lan: SocketAddr = "192.168.1.20:1234".parse().unwrap();
        assert!(is_loopback_peer(Some(&ipv4)));
        assert!(is_loopback_peer(Some(&ipv6)));
        assert!(!is_loopback_peer(Some(&lan)));
        assert!(!is_loopback_peer(None));
    }
}
