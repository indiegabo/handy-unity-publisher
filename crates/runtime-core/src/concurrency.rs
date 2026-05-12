//! Translates runtime concurrency settings into the scheduler policy enforced
//! by the local host.

use runtime_config::RuntimeConcurrencySettings;

/// Declares the host-local concurrency policy used by the runtime scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConcurrencyPolicy {
    pub max_concurrent_build_runs: u32,
    pub max_concurrent_publish_runs: u32,
    pub max_active_releases_per_repository: u32,
}

impl HostConcurrencyPolicy {
    /// Builds a host-local concurrency policy from runtime configuration.
    pub fn from_settings(settings: &RuntimeConcurrencySettings) -> Self {
        Self {
            max_concurrent_build_runs: settings.max_concurrent_build_runs,
            max_concurrent_publish_runs: settings.max_concurrent_publish_runs,
            max_active_releases_per_repository: settings.max_active_releases_per_repository,
        }
    }

    /// Returns whether the host may claim another build run.
    pub fn allows_build_claim(&self, active_build_runs_on_host: u32) -> bool {
        active_build_runs_on_host < self.max_concurrent_build_runs
    }

    /// Returns whether the host may claim another publish run.
    pub fn allows_publish_claim(&self, active_publish_runs_on_host: u32) -> bool {
        active_publish_runs_on_host < self.max_concurrent_publish_runs
    }

    /// Returns whether the repository may start another release build lane.
    pub fn allows_release_lane(&self, active_releases_for_repository: u32) -> bool {
        active_releases_for_repository < self.max_active_releases_per_repository
    }

    /// Returns the remaining build-run claim capacity on the local host.
    pub fn remaining_build_capacity(&self, active_build_runs_on_host: u32) -> u32 {
        self.max_concurrent_build_runs
            .saturating_sub(active_build_runs_on_host)
    }

    /// Returns the remaining publish-run claim capacity on the local host.
    pub fn remaining_publish_capacity(&self, active_publish_runs_on_host: u32) -> u32 {
        self.max_concurrent_publish_runs
            .saturating_sub(active_publish_runs_on_host)
    }
}

#[cfg(test)]
mod tests {
    use super::HostConcurrencyPolicy;
    use runtime_config::RuntimeConcurrencySettings;

    #[test]
    fn policy_defaults_to_single_host_worker_capacity() {
        let policy =
            HostConcurrencyPolicy::from_settings(&RuntimeConcurrencySettings::development());

        assert_eq!(policy.max_concurrent_build_runs, 1);
        assert_eq!(policy.max_concurrent_publish_runs, 1);
        assert_eq!(policy.max_active_releases_per_repository, 1);
    }

    #[test]
    fn build_claims_stop_at_host_capacity() {
        let policy = HostConcurrencyPolicy {
            max_concurrent_build_runs: 2,
            max_concurrent_publish_runs: 1,
            max_active_releases_per_repository: 1,
        };

        assert!(policy.allows_build_claim(0));
        assert!(policy.allows_build_claim(1));
        assert!(!policy.allows_build_claim(2));
        assert_eq!(policy.remaining_build_capacity(0), 2);
        assert_eq!(policy.remaining_build_capacity(1), 1);
        assert_eq!(policy.remaining_build_capacity(3), 0);
    }

    #[test]
    fn publish_claims_stop_at_host_capacity() {
        let policy = HostConcurrencyPolicy {
            max_concurrent_build_runs: 1,
            max_concurrent_publish_runs: 3,
            max_active_releases_per_repository: 1,
        };

        assert!(policy.allows_publish_claim(0));
        assert!(policy.allows_publish_claim(2));
        assert!(!policy.allows_publish_claim(3));
        assert_eq!(policy.remaining_publish_capacity(0), 3);
        assert_eq!(policy.remaining_publish_capacity(2), 1);
        assert_eq!(policy.remaining_publish_capacity(4), 0);
    }

    #[test]
    fn release_lane_stays_serialized_per_repository() {
        let policy = HostConcurrencyPolicy {
            max_concurrent_build_runs: 4,
            max_concurrent_publish_runs: 2,
            max_active_releases_per_repository: 1,
        };

        assert!(policy.allows_release_lane(0));
        assert!(!policy.allows_release_lane(1));
    }
}