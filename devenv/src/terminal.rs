//! Terminal capability checks.

use std::io::{self, IsTerminal};
use std::os::fd::AsFd;

/// Foreground-process-group checks for terminal-backed file descriptors.
pub trait IsForegroundTerminal {
    fn is_foreground_terminal(&self) -> bool;
}

impl<T: AsFd> IsForegroundTerminal for T {
    fn is_foreground_terminal(&self) -> bool {
        nix::unistd::tcgetpgrp(self).is_ok_and(|foreground| foreground == nix::unistd::getpgrp())
    }
}

/// Whether stdin can be used for interactive input and stderr for its UI.
pub fn can_use_stdin_interactively() -> bool {
    io::stdin().is_foreground_terminal() && io::stderr().is_terminal()
}

/// Whether the controlling terminal's foreground process group is ours.
pub fn controlling_terminal_is_foreground() -> bool {
    std::fs::File::open("/dev/tty").is_ok_and(|tty| tty.is_foreground_terminal())
}
