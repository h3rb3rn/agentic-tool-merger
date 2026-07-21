//! Core domain types and invariants shared across `SessionMesh`.

pub mod configuration;
pub mod event;

/// Identifies the initial, pre-feature workspace build.
///
/// This stable value gives bootstrap smoke tests a behavior to exercise without
/// introducing domain functionality before its dedicated implementation prompt.
#[must_use]
pub const fn bootstrap_stage() -> &'static str {
    "workspace-bootstrap"
}

#[cfg(test)]
mod tests {
    use super::bootstrap_stage;

    #[test]
    fn reports_the_workspace_bootstrap_stage() {
        assert_eq!(bootstrap_stage(), "workspace-bootstrap");
    }
}
