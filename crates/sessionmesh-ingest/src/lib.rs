//! Ingestion orchestration and native-source collection.

mod chunk;

/// Codex-specific read-only discovery.
pub mod codex;
pub mod external;
pub mod opencode;

pub use sessionmesh_core::bootstrap_stage;
