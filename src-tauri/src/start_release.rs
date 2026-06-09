use std::io;
use std::io::ErrorKind;

use runtime_config::RuntimeConfig;
use runtime_store::{
    read_unity_local_workspace_version,
    OnDemandReleaseRemoteRef,
    OnDemandReleaseRemoteRefsInput as StoreOnDemandReleaseRemoteRefsInput,
    OnDemandReleaseVersionPreviewInput as StoreOnDemandReleaseVersionPreviewInput,
    ReleaseRunRecord, StorageLayout,
};

use crate::{
    emit_shell_release_queued_event, normalize_on_demand_release_process_command_input,
    normalize_optional_shell_string, OnDemandReleaseProcessCommandInput,
    OnDemandReleaseRemoteRefsCommandInput,
    OnDemandReleaseVersionPreviewCommandInput,
};

struct StartReleaseRuntime {
    storage: StorageLayout,
    coordinator: runtime_store::LocalCoordinator,
}

impl StartReleaseRuntime {
    fn load(config: &RuntimeConfig, repository_id: i64) -> io::Result<Self> {
        config.directories.ensure_exists()?;
        let storage = StorageLayout::from_directories(&config.directories);
        if !storage.database_path.is_file() {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!("repository {repository_id} was not found"),
            ));
        }

        Ok(Self {
            coordinator: runtime_store::LocalCoordinator::new(&storage),
            storage,
        })
    }

    fn resolve_adapter(&self, source_kind: &str) -> io::Result<ResolvedStartReleaseAdapter<'_>> {
        Ok(match StartReleaseAdapterKind::resolve(source_kind)? {
            StartReleaseAdapterKind::LocalWorkspace => {
                ResolvedStartReleaseAdapter::Local(LocalWorkspaceStartReleaseAdapter {
                    runtime: self,
                })
            }
            StartReleaseAdapterKind::ManagedRepository => {
                ResolvedStartReleaseAdapter::Managed(ManagedRepositoryStartReleaseAdapter {
                    runtime: self,
                })
            }
        })
    }
}

enum StartReleaseAdapterKind {
    LocalWorkspace,
    ManagedRepository,
}

impl StartReleaseAdapterKind {
    fn resolve(source_kind: &str) -> io::Result<Self> {
        match source_kind.trim() {
            "local_workspace" => Ok(Self::LocalWorkspace),
            "managed_ref" | "managed_tag" => Ok(Self::ManagedRepository),
            "" => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "on-demand release source_kind must not be empty",
            )),
            other => Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("unsupported on-demand release source_kind {:?}", other),
            )),
        }
    }
}

enum ResolvedStartReleaseAdapter<'a> {
    Local(LocalWorkspaceStartReleaseAdapter<'a>),
    Managed(ManagedRepositoryStartReleaseAdapter<'a>),
}

impl ResolvedStartReleaseAdapter<'_> {
    fn dispatch(self, input: OnDemandReleaseProcessCommandInput) -> io::Result<ReleaseRunRecord> {
        match self {
            Self::Local(adapter) => adapter.dispatch(input),
            Self::Managed(adapter) => adapter.dispatch(input),
        }
    }

    fn preview_version(self, input: OnDemandReleaseVersionPreviewCommandInput) -> io::Result<String> {
        match self {
            Self::Local(adapter) => adapter.preview_version(input),
            Self::Managed(adapter) => adapter.preview_version(input),
        }
    }

    fn list_remote_refs(
        self,
        input: OnDemandReleaseRemoteRefsCommandInput,
    ) -> io::Result<Vec<OnDemandReleaseRemoteRef>> {
        match self {
            Self::Local(adapter) => adapter.list_remote_refs(input),
            Self::Managed(adapter) => adapter.list_remote_refs(input),
        }
    }
}

struct LocalWorkspaceStartReleaseAdapter<'a> {
    runtime: &'a StartReleaseRuntime,
}

impl LocalWorkspaceStartReleaseAdapter<'_> {
    fn dispatch(&self, input: OnDemandReleaseProcessCommandInput) -> io::Result<ReleaseRunRecord> {
        self.runtime
            .coordinator
            .dispatch_on_demand_release(normalize_on_demand_release_process_command_input(input)?)
    }

    fn preview_version(&self, input: OnDemandReleaseVersionPreviewCommandInput) -> io::Result<String> {
        let local_path = normalize_optional_shell_string(input.local_path).ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "local workspace path must not be empty",
            )
        })?;

        read_unity_local_workspace_version(&local_path)
    }

    fn list_remote_refs(
        &self,
        _input: OnDemandReleaseRemoteRefsCommandInput,
    ) -> io::Result<Vec<OnDemandReleaseRemoteRef>> {
        Err(io::Error::new(
            ErrorKind::InvalidInput,
            "local workspace start release does not expose remote refs",
        ))
    }
}

struct ManagedRepositoryStartReleaseAdapter<'a> {
    runtime: &'a StartReleaseRuntime,
}

impl ManagedRepositoryStartReleaseAdapter<'_> {
    fn dispatch(&self, input: OnDemandReleaseProcessCommandInput) -> io::Result<ReleaseRunRecord> {
        self.runtime
            .coordinator
            .dispatch_on_demand_release(normalize_on_demand_release_process_command_input(input)?)
    }

    fn preview_version(&self, input: OnDemandReleaseVersionPreviewCommandInput) -> io::Result<String> {
        self.runtime.coordinator.preview_on_demand_release_version(
            StoreOnDemandReleaseVersionPreviewInput {
                repository_id: input.repository_id,
                version_source: input.version_source.trim().to_owned(),
                source_kind: input.source_kind.trim().to_owned(),
                source_ref: normalize_optional_shell_string(input.source_ref),
                local_path: normalize_optional_shell_string(input.local_path),
            },
        )
    }

    fn list_remote_refs(
        &self,
        input: OnDemandReleaseRemoteRefsCommandInput,
    ) -> io::Result<Vec<OnDemandReleaseRemoteRef>> {
        self.runtime.coordinator.list_on_demand_release_remote_refs(
            StoreOnDemandReleaseRemoteRefsInput {
                repository_id: input.repository_id,
                source_kind: input.source_kind.trim().to_owned(),
            },
        )
    }
}

pub(crate) fn request_on_demand_release_process(
    config: &RuntimeConfig,
    input: OnDemandReleaseProcessCommandInput,
) -> io::Result<ReleaseRunRecord> {
    let runtime = StartReleaseRuntime::load(config, input.repository_id)?;
    let adapter = runtime.resolve_adapter(&input.source_kind)?;
    let record = adapter.dispatch(input)?;
    emit_shell_release_queued_event(&runtime.storage, &runtime.coordinator, &record)?;

    Ok(record)
}

pub(crate) fn preview_on_demand_release_version(
    config: &RuntimeConfig,
    input: OnDemandReleaseVersionPreviewCommandInput,
) -> io::Result<String> {
    let runtime = StartReleaseRuntime::load(config, input.repository_id)?;
    runtime.resolve_adapter(&input.source_kind)?.preview_version(input)
}

pub(crate) fn list_on_demand_release_remote_refs(
    config: &RuntimeConfig,
    input: OnDemandReleaseRemoteRefsCommandInput,
) -> io::Result<Vec<OnDemandReleaseRemoteRef>> {
    let runtime = StartReleaseRuntime::load(config, input.repository_id)?;
    runtime.resolve_adapter(&input.source_kind)?.list_remote_refs(input)
}