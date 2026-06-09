import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ManagedRepositoryStartReleaseAdapter } from "./ManagedRepositoryStartReleaseAdapter";
import type { RepositoryInspectionEntry } from "../../services/projects";

const {
  dispatchOnDemandReleaseProcessMock,
  listOnDemandReleaseRemoteRefsMock,
  previewOnDemandReleaseVersionMock,
} = vi.hoisted(() => ({
  dispatchOnDemandReleaseProcessMock: vi.fn(),
  listOnDemandReleaseRemoteRefsMock: vi.fn(),
  previewOnDemandReleaseVersionMock: vi.fn(),
}));

vi.mock("../../services/projects", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/projects")
  >("../../services/projects");

  return {
    ...actual,
    dispatchOnDemandReleaseProcess: dispatchOnDemandReleaseProcessMock,
    listOnDemandReleaseRemoteRefs: listOnDemandReleaseRemoteRefsMock,
    previewOnDemandReleaseVersion: previewOnDemandReleaseVersionMock,
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  dispatchOnDemandReleaseProcessMock.mockResolvedValue({
    git_tag: "v2.1.0",
    id: 17,
    repository_id: 11,
    status: "queued",
  });
  previewOnDemandReleaseVersionMock.mockResolvedValue("v2.0.0");
  listOnDemandReleaseRemoteRefsMock.mockImplementation(async (input) => {
    if (input.source_kind === "managed_tag") {
      return [
        { commit: "c3", name: "v2.10.0" },
        { commit: "c2", name: "v2.0.0" },
      ];
    }

    return [
      { commit: "c1", name: "main" },
      { commit: "c4", name: "develop" },
    ];
  });
});

describe("ManagedRepositoryStartReleaseAdapter", () => {
  it("shows the tag-value version source only for tag releases and previews it", async () => {
    render(
      <ManagedRepositoryStartReleaseAdapter
        onBack={vi.fn()}
        onCancel={vi.fn()}
        onQueued={vi.fn()}
        repository={buildRepository()}
      />,
    );

    expect(
      screen.queryByRole("option", { name: "Use selected tag value" }),
    ).not.toBeInTheDocument();

    await waitFor(() => {
      expect(listOnDemandReleaseRemoteRefsMock).toHaveBeenCalledWith({
        repository_id: 11,
        source_kind: "managed_ref",
      });
    });

    fireEvent.change(
      screen.getByRole("combobox", { name: "Repository source" }),
      {
        target: { value: "tag" },
      },
    );

    await waitFor(() => {
      expect(listOnDemandReleaseRemoteRefsMock).toHaveBeenLastCalledWith({
        repository_id: 11,
        source_kind: "managed_tag",
      });
    });

    expect(
      await screen.findByRole("option", { name: "Use selected tag value" }),
    ).toBeInTheDocument();

    const tagSelect = screen.getByRole("combobox", { name: "Tag" });
    await waitFor(() => {
      expect(
        within(tagSelect)
          .getAllByRole("option")
          .map((option) => option.textContent),
      ).toEqual(["v2.10.0", "v2.0.0"]);
    });

    fireEvent.change(tagSelect, {
      target: { value: "v2.0.0" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Version source" }), {
      target: { value: "source_tag" },
    });

    await waitFor(() => {
      expect(previewOnDemandReleaseVersionMock).toHaveBeenLastCalledWith({
        local_path: null,
        repository_id: 11,
        source_kind: "managed_tag",
        source_ref: "v2.0.0",
        version_source: "source_tag",
      });
    });

    await waitFor(() => {
      expect(
        screen.getByRole("textbox", { name: "Release version" }),
      ).toHaveValue("v2.0.0");
    });
  });

  it("dispatches a managed branch release with the selected branch and manual version", async () => {
    const onQueued = vi.fn();

    render(
      <ManagedRepositoryStartReleaseAdapter
        onBack={vi.fn()}
        onCancel={vi.fn()}
        onQueued={onQueued}
        repository={buildRepository()}
      />,
    );

    const branchSelect = screen.getByRole("combobox", { name: "Branch" });
    await waitFor(() => {
      expect(
        within(branchSelect)
          .getAllByRole("option")
          .map((option) => option.textContent),
      ).toEqual(["main (default)", "develop"]);
    });

    fireEvent.change(branchSelect, {
      target: { value: "develop" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Release version" }), {
      target: { value: "v2.1.0" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Queue Managed Release" }),
    );

    await waitFor(() => {
      expect(dispatchOnDemandReleaseProcessMock).toHaveBeenCalledWith({
        local_path: null,
        process_priority: "low",
        release_version: "v2.1.0",
        repository_id: 11,
        source_kind: "managed_ref",
        source_ref: "develop",
        unity_executable_path_override: null,
        version_source: "manual",
      });
    });

    await waitFor(() => {
      expect(onQueued).toHaveBeenCalledWith("v2.1.0", "Managed Demo");
    });
  });

  it("dispatches a managed release with the selected process priority", async () => {
    render(
      <ManagedRepositoryStartReleaseAdapter
        onBack={vi.fn()}
        onCancel={vi.fn()}
        onQueued={vi.fn()}
        repository={buildRepository()}
      />,
    );

    const branchSelect = screen.getByRole("combobox", { name: "Branch" });
    await waitFor(() => {
      expect(
        within(branchSelect)
          .getAllByRole("option")
          .map((option) => option.textContent),
      ).toEqual(["main (default)", "develop"]);
    });

    fireEvent.change(
      screen.getByRole("combobox", { name: "Release process priority" }),
      {
        target: { value: "high" },
      },
    );
    fireEvent.change(screen.getByRole("textbox", { name: "Release version" }), {
      target: { value: "v2.4.0" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Queue Managed Release" }),
    );

    await waitFor(() => {
      expect(dispatchOnDemandReleaseProcessMock).toHaveBeenCalledWith(
        expect.objectContaining({
          process_priority: "high",
          release_version: "v2.4.0",
        }),
      );
    });
  });
});

function buildRepository(): RepositoryInspectionEntry {
  return {
    repository_id: 11,
    repository_name: "Managed Demo",
    source_mode: "managed_repository",
    workspace_strategy: "managed_checkout",
    repo_url: "https://github.com/indiegabo/managed-demo.git",
    local_path: null,
    engine_kind: "unity",
    enabled: true,
    polling_interval_seconds: 60,
    default_branch: "main",
    artifacts_root_override: null,
    workspace_root_override: null,
    last_seen_tag: null,
    enabled_build_target_count: 1,
    credentials: null,
    source_provider_id: "github",
    source_instance_url: "https://github.com",
    visibility_status: "private",
    auth_requirement_status: "required",
    auth_binding_status: "connected",
    auth_status_message: "Connected",
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
