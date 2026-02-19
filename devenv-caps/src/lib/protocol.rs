use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Wire messages
// ---------------------------------------------------------------------------

/// How a child process exited.
#[derive(Debug, Serialize, Deserialize)]
pub enum ProcessExit {
    /// Process called `exit(code)`.
    Exited(i32),
    /// Process was terminated by a signal.
    Signaled(i32),
}

/// Information about a child process that has exited.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExitedProcess {
    /// PID of the exited process.
    pub pid: u32,
    /// How the process exited.
    pub exit: ProcessExit,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    /// Fork a child with the given capabilities, drop to the target user, exec.
    Launch {
        /// Logical process name (for logging / tracking).
        id: String,
        /// Capability names, lowercase without the `cap_` prefix.
        /// e.g. `["net_bind_service", "ipc_lock"]`
        caps: Vec<String>,
        /// Absolute path to the executable.
        command: String,
        /// Arguments to the executable.
        args: Vec<String>,
        /// Environment variables for the child.
        env: HashMap<String, String>,
        /// Working directory for the child.
        working_dir: PathBuf,
    },

    /// Send a signal to a previously launched process.
    Signal { pid: u32, signal: i32 },

    /// Poll for children that have exited since the last poll.
    Poll,

    /// Shut down the cap-server. Sends SIGTERM to all children first.
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// Process was successfully launched.
    Launched { pid: u32 },

    /// An error occurred.
    Error { message: String },

    /// Generic success (for Signal / Shutdown).
    Ok,

    /// Response to a Poll request: all children that exited since last poll.
    Exited { processes: Vec<ExitedProcess> },
}

// ---------------------------------------------------------------------------
// Length-prefixed JSON framing
// ---------------------------------------------------------------------------

/// Write a message as a 4-byte big-endian length prefix followed by JSON.
pub fn write_message<T: Serialize>(stream: &mut UnixStream, msg: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&payload)?;
    stream.flush()
}

/// Read a length-prefixed JSON message.
pub fn read_message<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message too large: {len} bytes"),
        ));
    }

    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
