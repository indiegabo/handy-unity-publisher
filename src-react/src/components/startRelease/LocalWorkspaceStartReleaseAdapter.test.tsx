import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LocalWorkspaceStartReleaseAdapter } from "./LocalWorkspaceStartReleaseAdapter";
import type { RepositoryInspectionEntry } from "../../services/projects";

const { dispatchOnDemandReleaseProcessMock, readProjectSettingsVersionMock } =
  vi.hoisted(() => ({
    dispatchOnDemandReleaseProcessMock: vi.fn(),
    readProjectSettingsVersionMock: vi.fn(),
  }));

vi.mock("../../services/projects", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/projects")
  >("../../services/projects");

  return {
    ...actual,
    dispatchOnDemandReleaseProcess: dispatchOnDemandReleaseProcessMock,
    readProjectSettingsVersion: readProjectSettingsVersionMock,
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  dispatchOnDemandReleaseProcessMock.mockResolvedValue({
    git_tag: "v1.3.0",
    id: 9,
    repository_id: 7,
    status: "queued",
  });
  readProjectSettingsVersionMock.mockResolvedValue("v1.3.0");
});

describe("LocalWorkspaceStartReleaseAdapter", () => {
  it("dispatches a local release with the selected process priority", async () => {
    const onQueued = vi.fn();

    render(
      <LocalWorkspaceStartReleaseAdapter
        onBack={vi.fn()}
        onCancel={vi.fn()}
        onQueued={onQueued}
        repository={buildRepository()}
      />,
    );

    fireEvent.change(
      screen.getByRole("combobox", { name: "Release process priority" }),
      {
        target: { value: "high" },
      },
    );
    fireEvent.change(screen.getByRole("textbox", { name: "Release version" }), {
      target: { value: "v1.3.0" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Queue Local Release" }),
    );

    await waitFor(() => {
      expect(dispatchOnDemandReleaseProcessMock).toHaveBeenCalledWith({
        local_path: "C:/projects/local-demo",
        process_priority: "high",
        release_version: "v1.3.0",
        repository_id: 7,
        source_kind: "local_workspace",
        source_ref: null,
        unity_executable_path_override: null,
        version_source: "manual",
      });
    });

    await waitFor(() => {
      expect(onQueued).toHaveBeenCalledWith("v1.3.0", "Local Demo");
    });
  });
});

function buildRepository(): RepositoryInspectionEntry {
  return {
    repository_id: 7,
    repository_name: "Local Demo",
    source_mode: "local_workspace",
    workspace_strategy: "direct_local_path",
    repo_url: "C:/projects/local-demo",
    local_path: "C:/projects/local-demo",
    engine_kind: "unity",
    enabled: true,
    polling_interval_seconds: 0,
    default_branch: null,
    artifacts_root_override: null,
    workspace_root_override: null,
    last_seen_tag: null,
    enabled_build_target_count: 1,
    credentials: null,
    source_provider_id: null,
    source_instance_url: null,
    visibility_status: "local",
    auth_requirement_status: "not_required",
    auth_binding_status: "not_applicable",
    auth_status_message: "Not required",
    auth_last_verified_at: null,
    build_targets: [],
    publish_targets: [],
    pending_release_count: 0,
    queued_build_runs: 0,
    running_build_runs: 0,
    queued_publish_runs: 0,
    running_publish_runs: 0,
    release_queue: [],
  };
}
