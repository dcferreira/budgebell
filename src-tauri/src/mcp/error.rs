use thiserror::Error;

use crate::domain::DomainError;
use crate::store::StoreError;

/// Errors surfaced by the MCP tool handlers (design spec §6). Kept
/// rmcp-agnostic so the handlers stay unit-testable without the SDK; the
/// server boundary (`server.rs`) maps these onto rmcp's `ErrorData`.
///
/// Validation is strict and loud — a bad category, a malformed trigger, or a
/// missing habit all surface here rather than being silently swallowed.
#[derive(Debug, Error)]
pub enum McpToolError {
    /// Domain-model validation rejected the request (empty name, zero
    /// weight, weekly count out of range, unparsable time, ...).
    #[error(transparent)]
    Domain(#[from] DomainError),

    /// The store rejected the operation (not found, database error, ...).
    #[error(transparent)]
    Store(#[from] StoreError),
}
