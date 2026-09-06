//! Shared HTTP reverse proxy for friendly devenv `.localhost` URLs.

mod certificates;
mod control;
mod routes;

#[cfg(feature = "server")]
use anyhow::{Context, Result};
#[cfg(feature = "server")]
use async_trait::async_trait;
#[cfg(feature = "server")]
use bytes::Bytes;
pub use certificates::TlsConfig;
#[cfg(feature = "server")]
pub use control::serve_control;
pub use control::{ControlRequest, ControlResponse, request};
#[cfg(feature = "server")]
use pingora_core::tls::{ext, ssl::NameType};
#[cfg(feature = "server")]
use pingora_core::{
    listeners::{ConnectionFilter, TlsAccept, tls::TlsSettings},
    protocols::tls::TlsRef,
    server::{Server, configuration::ServerConf},
    upstreams::peer::HttpPeer,
};
#[cfg(feature = "server")]
use pingora_proxy::{ProxyHttp, Session, http_proxy_service};
#[cfg(feature = "server")]
pub use routes::RouteTable;
pub use routes::{Route, normalize_hostname};
use std::{env, path::PathBuf};
#[cfg(feature = "server")]
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    sync::Arc,
    time::Duration,
};
#[cfg(all(feature = "server", target_os = "macos"))]
use std::{num::NonZeroU32, os::fd::IntoRawFd};

#[cfg(feature = "server")]
const NOT_FOUND: &[u8] = b"No devenv process is registered for this hostname.\n";
pub const HEALTH_HOSTNAME: &str = "_devenv-proxy.localhost";
#[cfg(feature = "server")]
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

#[cfg(feature = "server")]
struct Router {
    routes: RouteTable,
}

#[cfg(feature = "server")]
struct Certificates(RouteTable);

#[cfg(feature = "server")]
#[async_trait]
impl TlsAccept for Certificates {
    async fn certificate_callback(&self, ssl: &mut TlsRef) {
        let Some(certificate) = ssl
            .servername(NameType::HOST_NAME)
            .and_then(|hostname| self.0.certificate(hostname))
        else {
            return;
        };
        let mut install = || -> Result<()> {
            ext::ssl_use_certificate(ssl, certificate.leaf())?;
            ext::ssl_use_private_key(ssl, &certificate.key)?;
            for certificate in certificate.chain.iter().skip(1) {
                ext::ssl_add_chain_cert(ssl, certificate)?;
            }
            Ok(())
        };
        if let Err(error) = install() {
            eprintln!("failed to install proxy TLS certificate: {error:#}");
        }
    }
}

#[cfg(feature = "server")]
struct Upstream {
    address: SocketAddr,
    fallback: Option<SocketAddr>,
}

#[cfg(feature = "server")]
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
#[cfg(feature = "server")]
struct LoopbackOnly;

#[cfg(feature = "server")]
#[async_trait]
impl ConnectionFilter for LoopbackOnly {
    async fn should_accept(&self, address: Option<&SocketAddr>) -> bool {
        is_loopback_peer(address)
    }
}

#[cfg(feature = "server")]
fn is_loopback_peer(address: Option<&SocketAddr>) -> bool {
    address.is_some_and(|address| address.ip().is_loopback())
}

#[cfg(feature = "server")]
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
        let mut peer = HttpPeer::new(upstream.address, false, String::new());
        peer.options.connection_timeout = Some(UPSTREAM_CONNECT_TIMEOUT);
        Ok(Box::new(peer))
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
        let scheme = if session
            .as_downstream()
            .digest()
            .is_some_and(|digest| digest.ssl_digest.is_some())
        {
            "https"
        } else {
            "http"
        };
        upstream_request.insert_header("x-forwarded-proto", scheme)?;
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

#[cfg(feature = "server")]
fn request_host(session: &Session) -> Option<String> {
    if let Some(authority) = session.req_header().uri.authority() {
        return Some(authority.host().to_owned());
    }

    let host = session.req_header().headers.get("host")?.to_str().ok()?;
    Some(strip_port(host).to_owned())
}

#[cfg(feature = "server")]
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
#[cfg(all(feature = "server", unix))]
pub fn run(listen: SocketAddr, control_socket: &Path) -> Result<()> {
    run_with_https(listen, None, control_socket)
}

#[cfg(all(feature = "server", unix))]
pub fn run_with_https(
    listen: SocketAddr,
    https_listen: Option<SocketAddr>,
    control_socket: &Path,
) -> Result<()> {
    let mut routes = RouteTable::default();
    routes.https_listen = https_listen;
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

    let certificates = Certificates(routes.clone());
    let mut service = http_proxy_service(&server.configuration, Router { routes });
    service.set_connection_filter(Arc::new(LoopbackOnly));

    #[cfg(target_os = "macos")]
    let mut prebound = Vec::new();
    #[cfg(target_os = "macos")]
    let http_bind = listener_bind(listen, &mut prebound)?;
    #[cfg(not(target_os = "macos"))]
    let http_bind = listen.to_string();
    service.add_tcp(&http_bind);

    if let Some(listen) = https_listen {
        let settings = TlsSettings::with_callbacks(Box::new(certificates))
            .context("failed to configure proxy TLS")?;
        #[cfg(target_os = "macos")]
        let bind = listener_bind(listen, &mut prebound)?;
        #[cfg(not(target_os = "macos"))]
        let bind = listen.to_string();
        service.add_tls_with_settings(&bind, None, settings);
    }

    #[cfg(target_os = "macos")]
    server.add_service(PreboundService {
        inner: service,
        prebound,
    });

    #[cfg(not(target_os = "macos"))]
    {
        server.add_service(service);
    }
    server.run_forever();
}

#[cfg(all(feature = "server", target_os = "macos"))]
struct PreboundListener {
    bind: String,
    socket: socket2::Socket,
}

#[cfg(all(feature = "server", target_os = "macos"))]
fn listener_bind(listen: SocketAddr, prebound: &mut Vec<PreboundListener>) -> Result<String> {
    if let Some(listener) = prebind_macos_low_port(listen)? {
        let bind = listener.bind.clone();
        prebound.push(listener);
        Ok(bind)
    } else {
        Ok(listen.to_string())
    }
}

/// Bind a low port without privilege by using Darwin's wildcard exception,
/// while `IP_BOUND_IF`/`IPV6_BOUND_IF` restricts the socket to `lo0` in the
/// kernel. Pingora adopts this descriptor through its inherited-FD table.
#[cfg(all(feature = "server", target_os = "macos"))]
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

#[cfg(all(feature = "server", target_os = "macos"))]
struct PreboundService<A> {
    inner: pingora_core::services::listening::Service<A>,
    prebound: Vec<PreboundListener>,
}

#[cfg(all(feature = "server", target_os = "macos"))]
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
        for prebound in self.prebound.drain(..) {
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

#[cfg(all(feature = "server", not(unix)))]
pub fn run(_listen: SocketAddr, _control_socket: &Path) -> Result<()> {
    anyhow::bail!("devenv proxy currently requires Unix domain sockets")
}

#[cfg(all(test, feature = "server"))]
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
