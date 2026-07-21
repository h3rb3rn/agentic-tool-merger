//! Codex installation and native-session source support.

mod discovery;
mod incremental;
mod parser;

pub use discovery::{
    CodexDiscoveryInput, CodexDiscoveryReport, CodexHome, CodexInstallation, CodexSource,
    CodexSourceKind, CodexSurface, DiscoveryDiagnostic, DiscoverySeverity, HomeOrigin,
    SourcePermissions, discover,
};
pub use incremental::{
    IncrementalInput, IngestError, OrderingKey, PreparedIncremental, ResetReason, RetryDecision,
    RetryPolicy, WatchDebouncer, ingest_file, ordering_key, prepare_incremental,
};
pub use parser::{CodexParseContext, ParseIssue, ParsedRecord, RawRolloutRecord, parse_rollout};
