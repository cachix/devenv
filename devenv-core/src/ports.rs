//! Port allocation helpers for devenv processes.
//!
//! Provides utilities for finding available ports, used by the Nix backend's
//! `builtins.devenvAllocatePort` primop.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Default host for port allocation (localhost only).
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Maximum number of ports to try before giving up.
pub const MAX_ATTEMPTS: u16 = 100;

/// Guard holding port reservations. Releases ports when dropped.
///
/// This implements RAII-style port management: ports are held (via bound TcpListeners)
/// until this guard is dropped, at which point the listeners are closed and the
/// ports become available for processes to bind.
pub struct PortReservations {
    ports: HashMap<u16, TcpListener>,
}

impl PortReservations {
    pub fn new(ports: HashMap<u16, TcpListener>) -> Self {
        Self { ports }
    }

    /// Get the list of reserved ports
    pub fn ports(&self) -> Vec<u16> {
        self.ports.keys().copied().collect()
    }

    /// Check if any ports are reserved
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }
}

impl Drop for PortReservations {
    fn drop(&mut self) {
        if !self.ports.is_empty() {
            tracing::debug!(
                "Releasing {} port reservations: {:?}",
                self.ports.len(),
                self.ports.keys().collect::<Vec<_>>()
            );
        }
        // TcpListeners are dropped here, releasing the ports
    }
}

/// Entry for an allocated port, containing the port number and optional listener.
/// The listener is taken when processes start, but the port value remains for caching.
struct PortEntry {
    port: u16,
    listener: Option<TcpListener>,
}

/// Thread-safe port allocator that holds reservations until released.
///
/// Used to allocate ports during Nix evaluation and hold them until
/// the process manager starts, preventing race conditions where another
/// process could grab the port between allocation and use.
///
/// Allocations are keyed by (process_name, port_name) to ensure stable values
/// across multiple Nix evaluations in the same session.
pub struct PortAllocator {
    host: String,
    /// Allocated ports: (process_name, port_name) -> PortEntry
    ports: Mutex<HashMap<(String, String), PortEntry>>,
    /// When true, fail if the requested port is in use instead of finding the next available.
    strict: AtomicBool,
}

impl PortAllocator {
    pub fn new() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            ports: Mutex::new(HashMap::new()),
            strict: AtomicBool::new(false),
        }
    }

    /// Enable or disable strict mode.
    ///
    /// When strict mode is enabled, port allocation will fail with an error
    /// if the requested port is already in use, instead of automatically
    /// finding the next available port.
    pub fn set_strict(&self, strict: bool) {
        self.strict.store(strict, Ordering::SeqCst);
    }

    /// Check if strict mode is enabled.
    pub fn is_strict(&self) -> bool {
        self.strict.load(Ordering::SeqCst)
    }

    /// Allocate a port for a process, with caching by (process_name, port_name).
    ///
    /// If this (process_name, port_name) pair was already allocated, returns the
    /// cached port value. Otherwise, finds an available port starting from base.
    ///
    /// The port is held via a TcpListener until `take_reservations()` is called.
    ///
    /// In strict mode, only tries the base port and fails with process info if unavailable.
    pub fn allocate(&self, process_name: &str, port_name: &str, base: u16) -> Result<u16, String> {
        let mut ports = self.ports.lock().map_err(|e| e.to_string())?;
        let key = (process_name.to_string(), port_name.to_string());

        // Check cache first - return existing allocation if present
        if let Some(entry) = ports.get(&key) {
            return Ok(entry.port);
        }

        let strict = self.strict.load(Ordering::SeqCst);

        // Collect already-allocated port numbers to avoid conflicts
        let allocated_ports: std::collections::HashSet<u16> =
            ports.values().map(|e| e.port).collect();

        // In strict mode, only try the exact port requested
        if strict {
            // Check if already allocated to another process in this session
            if allocated_ports.contains(&base) {
                return Err(format!(
                    "Port {} is already in use by another process in this devenv session. \
                     Use --strict-ports=false to auto-allocate an available port.",
                    base
                ));
            }

            let addr = format!("{}:{}", self.host, base);
            match TcpListener::bind(&addr) {
                Ok(listener) => {
                    ports.insert(
                        key,
                        PortEntry {
                            port: base,
                            listener: Some(listener),
                        },
                    );
                    return Ok(base);
                }
                Err(_) => {
                    let process_info = get_process_using_port(base);
                    return Err(format!(
                        "Port {} is already in use{}. \
                         Use --strict-ports=false to auto-allocate an available port.",
                        base, process_info
                    ));
                }
            }
        }

        // Normal mode: try ports starting from base
        for offset in 0..MAX_ATTEMPTS {
            let Some(port) = base.checked_add(offset) else {
                // Port space exhausted (base + offset > 65535)
                break;
            };

            // Skip if already allocated in this session
            if allocated_ports.contains(&port) {
                continue;
            }

            let addr = format!("{}:{}", self.host, port);
            if let Ok(listener) = TcpListener::bind(&addr) {
                ports.insert(
                    key,
                    PortEntry {
                        port,
                        listener: Some(listener),
                    },
                );
                return Ok(port);
            }
        }

        Err(format!(
            "Could not find available port starting from {} after {} attempts",
            base, MAX_ATTEMPTS
        ))
    }

    /// Take all port reservations, returning a guard that releases them on drop.
    ///
    /// Extracts the TcpListeners but keeps the port allocations cached so that
    /// subsequent evaluations return the same port values.
    ///
    /// Call this just before spawning the process manager so ports are released
    /// right before use.
    pub fn take_reservations(&self) -> PortReservations {
        let mut ports = self.ports.lock().unwrap();

        // Extract listeners while keeping entries for caching
        let listeners: HashMap<u16, TcpListener> = ports
            .values_mut()
            .filter_map(|entry| entry.listener.take().map(|l| (entry.port, l)))
            .collect();

        PortReservations::new(listeners)
    }
}

impl Default for PortAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Try to find the process using a given port.
///
/// Returns a formatted string with process information, or an empty string if unknown.
fn get_process_using_port(port: u16) -> String {
    use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, get_sockets_info};

    let af_flags = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto_flags = ProtocolFlags::TCP;

    let Ok(sockets) = get_sockets_info(af_flags, proto_flags) else {
        return String::new();
    };

    for socket in sockets {
        let local_port = match &socket.protocol_socket_info {
            ProtocolSocketInfo::Tcp(tcp) => tcp.local_port,
            ProtocolSocketInfo::Udp(udp) => udp.local_port,
        };

        if local_port == port {
            if let Some(&pid) = socket.associated_pids.first() {
                // Try to get process name from /proc on Linux
                #[cfg(target_os = "linux")]
                if let Ok(name) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
                    return format!(" by {} (PID {})", name.trim(), pid);
                }

                return format!(" (PID {})", pid);
            }
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_allocator_basic() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate("server", "http", 49152).unwrap();
        assert!(port >= 49152);
    }

    #[test]
    fn test_port_allocator_skips_already_allocated() {
        let allocator = PortAllocator::new();

        // Allocate first port for server1
        let port1 = allocator.allocate("server1", "http", 49200).unwrap();
        assert!(port1 >= 49200);

        // Second allocation for different process from same base should get different port
        let port2 = allocator.allocate("server2", "http", 49200).unwrap();
        assert_ne!(port1, port2, "Allocations should get different ports");
    }

    #[test]
    fn test_port_allocator_caching() {
        let allocator = PortAllocator::new();

        // First allocation
        let port1 = allocator.allocate("server1", "http", 49200).unwrap();
        assert!(port1 >= 49200);

        // Same process+port should return cached value
        let port1_again = allocator.allocate("server1", "http", 49200).unwrap();
        assert_eq!(port1_again, port1);

        // Different process should get different port
        let port2 = allocator.allocate("server2", "http", 49200).unwrap();
        assert_ne!(port1, port2, "Different process should get different port");

        // Cached value persists even after take_reservations
        drop(allocator.take_reservations());
        let port1_cached = allocator.allocate("server1", "http", 49200).unwrap();
        assert_eq!(port1_cached, port1);
    }

    #[test]
    fn test_port_allocator_take_reservations() {
        let allocator = PortAllocator::new();

        let port1 = allocator.allocate("server1", "http", 49300).unwrap();
        let port2 = allocator.allocate("server2", "http", 49400).unwrap();

        let reservations = allocator.take_reservations();
        assert!(!reservations.is_empty());

        let ports = reservations.ports();
        assert!(ports.contains(&port1));
        assert!(ports.contains(&port2));

        // After taking, listeners are gone but cache remains
        let empty = allocator.take_reservations();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_port_released_after_drop() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate("server", "http", 49500).unwrap();

        // Port should be held
        let addr = format!("{}:{}", DEFAULT_HOST, port);
        assert!(TcpListener::bind(&addr).is_err());

        // Take and drop reservations
        drop(allocator.take_reservations());

        // Port should now be available
        assert!(TcpListener::bind(&addr).is_ok());
    }
}
