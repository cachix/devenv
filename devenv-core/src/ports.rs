//! Port allocation helpers for devenv processes.
//!
//! Provides utilities for finding available ports, used by the Nix backend's
//! `builtins.devenvAllocatePort` primop.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;

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

/// Thread-safe port allocator that holds reservations until released.
///
/// Used to allocate ports during Nix evaluation and hold them until
/// the process manager starts, preventing race conditions where another
/// process could grab the port between allocation and use.
pub struct PortAllocator {
    host: String,
    ports: Mutex<HashMap<u16, TcpListener>>,
}

impl PortAllocator {
    pub fn new() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            ports: Mutex::new(HashMap::new()),
        }
    }

    /// Allocate a port starting from base, incrementing until one is available.
    ///
    /// The port is held via a TcpListener until `take_reservations()` is called.
    pub fn allocate(&self, base: u16) -> Result<u16, String> {
        let mut ports = self.ports.lock().map_err(|e| e.to_string())?;

        for offset in 0..MAX_ATTEMPTS {
            let port = base.saturating_add(offset);

            // Skip if already allocated in this session
            if ports.contains_key(&port) {
                continue;
            }

            let addr = format!("{}:{}", self.host, port);
            if let Ok(listener) = TcpListener::bind(&addr) {
                ports.insert(port, listener);
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
    /// This drains the allocator's internal state. Call this just before
    /// spawning the process manager so ports are released right before use.
    pub fn take_reservations(&self) -> PortReservations {
        let mut ports = self.ports.lock().unwrap();
        PortReservations::new(std::mem::take(&mut *ports))
    }
}

impl Default for PortAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_allocator_basic() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate(49152).unwrap();
        assert!(port >= 49152);
    }

    #[test]
    fn test_port_allocator_skips_already_allocated() {
        let allocator = PortAllocator::new();

        // Allocate first port
        let port1 = allocator.allocate(49200).unwrap();
        assert_eq!(port1, 49200);

        // Second allocation from same base should get next port
        let port2 = allocator.allocate(49200).unwrap();
        assert_eq!(port2, 49201);
    }

    #[test]
    fn test_port_allocator_take_reservations() {
        let allocator = PortAllocator::new();

        let port1 = allocator.allocate(49300).unwrap();
        let port2 = allocator.allocate(49400).unwrap();

        let reservations = allocator.take_reservations();
        assert!(!reservations.is_empty());

        let ports = reservations.ports();
        assert!(ports.contains(&port1));
        assert!(ports.contains(&port2));

        // After taking, allocator should be empty
        let empty = allocator.take_reservations();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_port_released_after_drop() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate(49500).unwrap();

        // Port should be held
        let addr = format!("{}:{}", DEFAULT_HOST, port);
        assert!(TcpListener::bind(&addr).is_err());

        // Take and drop reservations
        drop(allocator.take_reservations());

        // Port should now be available
        assert!(TcpListener::bind(&addr).is_ok());
    }
}
