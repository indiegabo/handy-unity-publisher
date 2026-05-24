import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RepositoryInspectionEntry } from "../../services/projects";

const { loadLocalizationSettingsMock, readHostTextFileMock } = vi.hoisted(
  () => ({
    loadLocalizationSettingsMock: vi.fn(),
    readHostTextFileMock: vi.fn(),
  }),
);

vi.mock("../../services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
}));

vi.mock("../../services/processDetail", () => ({
  readHostTextFile: readHostTextFileMock,
}));

import { LocalizationProvider } from "../../LocalizationProvider";
import ProjectCard from "./ProjectCard";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadLocalizationSettingsMock.mockResolvedValue({
    available_locales: [
      {
        code: "en",
        display_name: "English",
        is_official: true,
        message_count: 20,
        native_name: "English",
      },
      {
        code: "pt-BR",
        display_name: "Português (Brasil)",
        is_official: true,
        message_count: 20,
        native_name: "Português (Brasil)",
      },
    ],
    fallback_locale: "en",
    localization_root: "C:/hgp/localizations",
    primary_locale: "pt-BR",
    warnings: [],
  });

  readHostTextFileMock.mockImplementation(async (path: string) => {
    if (path.endsWith("en.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "projects.presentation.source.managed_repository":
              "Managed repository",
            "projects.presentation.source.display":
              "{{sourceLabel}} · {{sourceValue}}",
            "projects.presentation.mode.managed_checkout": "Managed checkout",
            "projects.presentation.sync.cadence": "{{seconds}}s cadence",
            "projects.card.actions.open_project_named":
              "Open project {{repositoryName}}",
            "projects.card.actions.quick_view_named":
              "Quick view for {{repositoryName}}",
            "projects.card.actions.quick_view": "Quick view",
            "projects.card.badges.new": "new",
            "projects.card.direction.edit": "Edit",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "projects.presentation.source.managed_repository":
              "Repositório gerenciado",
            "projects.presentation.source.display":
              "{{sourceLabel}} · {{sourceValue}}",
            "projects.presentation.mode.managed_checkout":
              "Checkout gerenciado",
            "projects.presentation.sync.cadence": "{{seconds}}s de cadência",
            "projects.card.actions.open_project_named":
              "Abrir projeto {{repositoryName}}",
            "projects.card.actions.quick_view_named":
              "Visão rápida de {{repositoryName}}",
            "projects.card.actions.quick_view": "Visão rápida",
            "projects.card.badges.new": "novo",
            "projects.card.direction.edit": "Editar",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });
});

describe("ProjectCard", () => {
  it("renders translated project-card copy from the official locale pack", async () => {
    render(
      <LocalizationProvider>
        <ProjectCard
          highlighted
          onOpen={vi.fn()}
          onQuickView={vi.fn()}
          repository={buildRepository()}
        />
      </LocalizationProvider>,
    );

    expect(
      await screen.findByText(
        "Repositório gerenciado · https://github.com/indiegabo/worker-demo.git",
      ),
    ).toBeInTheDocument();
    expect(await screen.findByText("novo")).toBeInTheDocument();
    expect(await screen.findByText("Editar")).toBeInTheDocument();
    expect(await screen.findByText("Checkout gerenciado")).toBeInTheDocument();
    expect(await screen.findByText(/30s de cadência/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Abrir projeto Worker Demo" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Visão rápida de Worker Demo" }),
    ).toBeInTheDocument();
  });
});

function buildRepository(): RepositoryInspectionEntry {
  return {
    artifacts_root_override: null,
    auth_binding_status: "connected",
    auth_last_verified_at: "2026-05-19T00:00:00Z",
    auth_requirement_status: "required",
    auth_status_message: "GitHub access is connected.",
    build_targets: [
      {
        build_target_id: 11,
        diagnostic_message: "Ready for host-native execution.",
        diagnostic_status: "ready",
        enabled: true,
        host_native_diagnostics: null,
        repository_id: 1,
        repository_name: "Worker Demo",
        runner_type: "host-native",
        target_name: "Windows Build",
        unity_build_method: "Builder.PerformBuild",
        unity_target_platform: "StandaloneWindows64",
      },
    ],
    credentials: null,
    default_branch: "main",
    enabled: true,
    enabled_build_target_count: 1,
    engine_kind: "unity",
    last_seen_tag: "v0.1.0",
    local_path: null,
    pending_release_count: 0,
    polling_interval_seconds: 30,
    publish_targets: [],
    queued_build_runs: 0,
    queued_publish_runs: 0,
    release_queue: [],
    repo_url: "https://github.com/indiegabo/worker-demo.git",
    repository_id: 1,
    repository_name: "Worker Demo",
    running_build_runs: 0,
    running_publish_runs: 0,
    source_mode: "managed_repository",
    source_instance_url: "https://github.com",
    source_provider_id: "github",
    visibility_status: "private",
    workspace_root_override: null,
  };
}

function buildHostTextFilePayload(path: string, content: string) {
  return {
    content,
    exists: true,
    path,
    size_bytes: content.length,
    truncated: false,
  };
}
