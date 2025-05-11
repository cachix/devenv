//! Task Server Protocol implementation
//!
//! This module implements the Task Server Protocol (TSP) as proposed in
//! [GitHub issue #1457](https://github.com/cachix/devenv/issues/1457).
//!
//! The protocol allows defining tasks in any language using JSON-RPC,
//! enabling flexible task and process management within devenv.

pub mod client;
pub mod protocol;
pub mod sdk;
pub mod server;
