//! Resolves explicit engine adapters so generic orchestration can delegate
//! through one shared registry surface instead of scattering engine selection.

use runtime_contracts::EngineKind;
use std::io;
use std::io::ErrorKind;

/// Identifies the registered adapter that can execute one build plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildExecutionAdapter {
    Unity,
}

impl BuildExecutionAdapter {
    /// Returns the engine kind handled by this registered adapter.
    pub const fn engine_kind(self) -> EngineKind {
        match self {
            Self::Unity => EngineKind::Unity,
        }
    }
}

/// Resolves the current set of engine adapters exposed by the runtime runner layer.
#[derive(Debug, Clone, Copy, Default)]
pub struct EngineAdapterRegistry;

impl EngineAdapterRegistry {
    /// Creates the default adapter registry backed by the bundled runtime adapters.
    pub const fn new() -> Self {
        Self
    }

    /// Resolves the registered build execution adapter for one repository engine.
    pub fn resolve_build_execution_adapter(
        self,
        engine_kind: EngineKind,
    ) -> io::Result<BuildExecutionAdapter> {
        match engine_kind {
            EngineKind::Unity => Ok(BuildExecutionAdapter::Unity),
            other => Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "unsupported repository engine_kind {:?} for build execution adapter registry",
                    other.as_str()
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildExecutionAdapter, EngineAdapterRegistry};
    use runtime_contracts::EngineKind;
    use std::io::ErrorKind;

    #[test]
    fn resolve_build_execution_adapter_returns_unity_adapter() {
        let resolved = EngineAdapterRegistry::new()
            .resolve_build_execution_adapter(EngineKind::Unity)
            .expect("Unity should resolve to the bundled adapter");

        assert_eq!(resolved, BuildExecutionAdapter::Unity);
        assert_eq!(resolved.engine_kind(), EngineKind::Unity);
    }

    #[test]
    fn resolve_build_execution_adapter_rejects_unimplemented_engine() {
        let error = EngineAdapterRegistry::new()
            .resolve_build_execution_adapter(EngineKind::Godot)
            .expect_err("unimplemented engines should be rejected by the registry");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains("unsupported repository engine_kind \"godot\"")
        );
    }
}