//! The in-process MCP server (design spec §6): a local-transport-only rmcp
//! server through which a locally-running LLM manages the app's habits. The
//! tool surface maps directly onto the store operations (§5) and the domain
//! model (§4), and validation is strict — failures are loud, never silent.
//!
//! The design keeps the impure surface thin: the tool handlers ([`handlers`])
//! are pure functions over a [`Store`](crate::store::Store), exhaustively
//! tested against an in-memory database, and only [`server`] touches the rmcp
//! SDK, the async runtime, and the transport.

pub mod dto;
pub mod error;
pub mod handlers;
pub mod server;

pub use error::McpToolError;
pub use server::{serve_stdio, BudgebellServer};
