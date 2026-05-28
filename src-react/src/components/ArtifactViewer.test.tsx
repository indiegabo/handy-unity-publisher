import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ArtifactViewer } from "./ArtifactViewer";

afterEach(() => {
  cleanup();
});

describe("ArtifactViewer", () => {
  it("renders artifact summary metadata through the shared summary strip", () => {
    render(
      <ArtifactViewer
        artifact={buildArtifact()}
        artifactAbsolutePath="C:/tmp/artifacts/build.zip"
        artifactFolderPath="C:/tmp/artifacts"
        artifactLocationSummary="Filesystem path"
        resolvePublishTargetKindTone={() => "muted"}
      />,
    );

    expect(screen.getByText("Windows Build")).toBeInTheDocument();
    expect(screen.getByText("0 publishes")).toBeInTheDocument();
    expect(screen.getByText("Active location")).toBeInTheDocument();
    expect(screen.getByText("Size")).toBeInTheDocument();
  });

  it("autofocuses the folder action when the artifact file cannot be opened", async () => {
    const onOpenFolder = vi.fn();

    render(
      <ArtifactViewer
        artifact={buildArtifact()}
        artifactAbsolutePath={null}
        artifactFolderPath="C:/tmp/artifacts"
        artifactLocationSummary="Filesystem path"
        onOpenFolder={onOpenFolder}
        resolvePublishTargetKindTone={() => "muted"}
      />,
    );

    const openFolderButton = screen.getByRole("button", {
      name: "Open folder",
    });

    await waitFor(() => {
      expect(openFolderButton).toHaveFocus();
    });

    fireEvent.click(openFolderButton);

    expect(onOpenFolder).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "Open artifact" }),
    ).toBeDisabled();
  });
});

function buildArtifact() {
  return {
    artifact_active_location_kind: "filesystem_absolute",
    artifact_active_location_ref: "C:/tmp/artifacts/build.zip",
    artifact_id: 101,
    artifact_kind: "build-archive",
    artifact_name: "build.zip",
    artifact_path: "build.zip",
    artifact_root_path: "C:/tmp/artifacts",
    build_run_id: 11,
    build_status: "succeeded",
    build_target_id: 5,
    build_target_name: "Windows Build",
    canceled_publish_runs: 0,
    checksum_sha256: null,
    created_at: "2026-05-19T00:08:10Z",
    failed_publish_runs: 0,
    git_commit: "abc1234",
    git_tag: "v0.1.0",
    publish_run_count: 0,
    publish_runs: [],
    queued_publish_runs: 0,
    release_run_id: 77,
    repository_id: 1,
    repository_name: "Worker Demo",
    repository_url: "https://github.com/indiegabo/worker-demo.git",
    running_publish_runs: 0,
    size_bytes: 2048,
    succeeded_publish_runs: 0,
    unity_target_platform: "StandaloneWindows64",
    runner_type: "local",
  };
}
