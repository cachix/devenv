#![cfg(unix)]

use devenv_proxy::{ControlRequest, Route, request};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const HOSTNAME: &str = "docs.test.localhost";
const BODY: &str = "loopback request body";

struct Proxy {
    child: Child,
    address: SocketAddr,
    directory: tempfile::TempDir,
}

impl Proxy {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_devenv-proxy"))
            .args(["--listen", &address.to_string(), "--control-socket"])
            .arg(directory.path().join("proxy.sock"))
            .spawn()
            .unwrap();
        let mut proxy = Self {
            child,
            address,
            directory,
        };
        proxy.wait_until_ready();
        proxy
    }

    fn wait_until_ready(&mut self) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(self.child.try_wait().unwrap().is_none(), "proxy exited");
            if self.directory.path().join("proxy.sock").exists()
                && TcpStream::connect_timeout(&self.address, TIMEOUT).is_ok()
            {
                return;
            }
            assert!(Instant::now() < deadline, "proxy did not start");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn send(&self, upstream: SocketAddr) -> TcpStream {
        request(
            &self.directory.path().join("proxy.sock"),
            &ControlRequest::Register {
                route: Route {
                    hostname: HOSTNAME.to_owned(),
                    upstream,
                    owner: "test".to_owned(),
                },
            },
        )
        .unwrap()
        .into_result()
        .unwrap();
        let mut stream = TcpStream::connect_timeout(&self.address, TIMEOUT).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        write!(
            stream,
            "POST / HTTP/1.1\r\nHost: {HOSTNAME}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{BODY}",
            BODY.len()
        )
        .unwrap();
        stream
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Keep both families bound so an unrelated server cannot claim the unused
// address. A bound socket that isn't listening still refuses connections.
fn loopback_sockets() -> (Socket, Socket, u16) {
    let ipv4 = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    ipv4.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, 0)).into())
        .unwrap();
    let port = ipv4.local_addr().unwrap().as_socket().unwrap().port();
    let ipv6 = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();
    ipv6.set_only_v6(true).unwrap();
    ipv6.bind(&SocketAddr::from((Ipv6Addr::LOCALHOST, port)).into())
        .unwrap();
    (ipv4, ipv6, port)
}

fn respond(listener: &TcpListener) {
    let deadline = Instant::now() + TIMEOUT;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "upstream received no request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("upstream accept failed: {error}"),
        }
    };
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(headers.starts_with("POST / HTTP/1.1\r\n"));
    assert!(
        headers
            .to_ascii_lowercase()
            .contains(&format!("host: {HOSTNAME}\r\n"))
    );
    let mut body = vec![0; BODY.len()];
    reader.read_exact(&mut body).unwrap();
    assert_eq!(body, BODY.as_bytes());
    reader
        .get_mut()
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .unwrap();
}

#[test]
fn proxies_to_either_loopback_family_and_prefers_the_registered_address() {
    let proxy = Proxy::start();
    for (register_ipv6, listen_ipv4, listen_ipv6) in [
        (false, true, false),
        (false, false, true),
        (true, true, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let (ipv4, ipv6, port) = loopback_sockets();
        if listen_ipv4 {
            ipv4.listen(8).unwrap();
        }
        if listen_ipv6 {
            ipv6.listen(8).unwrap();
        }
        let ipv4: TcpListener = ipv4.into();
        let ipv6: TcpListener = ipv6.into();
        ipv4.set_nonblocking(true).unwrap();
        ipv6.set_nonblocking(true).unwrap();
        let upstream = if register_ipv6 {
            SocketAddr::from((Ipv6Addr::LOCALHOST, port))
        } else {
            SocketAddr::from((Ipv4Addr::LOCALHOST, port))
        };
        let mut client = proxy.send(upstream);
        let expected_ipv6 = listen_ipv6 && (register_ipv6 || !listen_ipv4);
        respond(if expected_ipv6 { &ipv6 } else { &ipv4 });
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("\r\n\r\nok"), "{response}");
        if listen_ipv4 && listen_ipv6 {
            let unused = if expected_ipv6 { &ipv4 } else { &ipv6 };
            assert_eq!(
                unused.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    }
}

#[test]
fn returns_502_when_neither_loopback_address_is_listening() {
    let proxy = Proxy::start();
    let (_ipv4, _ipv6, port) = loopback_sockets();
    let mut client = proxy.send(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
}

#[test]
fn sigterm_releases_the_daemon_for_restart() {
    let mut proxy = Proxy::start();
    assert!(
        Command::new("kill")
            .args(["-TERM", &proxy.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = proxy.child.try_wait().unwrap() {
            assert!(status.success(), "proxy shutdown failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "proxy did not finish shutdown");
        thread::sleep(Duration::from_millis(10));
    }

    // Reuse both addresses, including the stale Unix socket left on disk.
    proxy.child = Command::new(env!("CARGO_BIN_EXE_devenv-proxy"))
        .args(["--listen", &proxy.address.to_string(), "--control-socket"])
        .arg(proxy.directory.path().join("proxy.sock"))
        .spawn()
        .unwrap();
    proxy.wait_until_ready();
    let (ipv4, _ipv6, port) = loopback_sockets();
    ipv4.listen(8).unwrap();
    let listener: TcpListener = ipv4.into();
    listener.set_nonblocking(true).unwrap();
    let mut client = proxy.send(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    respond(&listener);
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}
