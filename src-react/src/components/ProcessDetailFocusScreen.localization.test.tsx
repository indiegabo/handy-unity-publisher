import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OverlayProvider from "./OverlayManager";
import { ProcessDetailFocusScreen } from "./ProcessDetailFocusScreen";
import type { ProcessFeedRecord } from "./processFeedPresentation";

const {
  deleteReleaseProcessOutputsMock,
  loadArtifactInspectionMock,
  loadBuildExecutionReportMock,
  loadBuildHistoryMock,
  loadLocalizationSettingsMock,
  openHostPathMock,
  purgeBuildExecutionRetentionMock,
  readHostTextFileMock,
  readRetainedLogArchiveEntryMock,
} = vi.hoisted(() => ({
  deleteReleaseProcessOutputsMock: vi.fn(),
  loadArtifactInspectionMock: vi.fn(),
  loadBuildExecutionReportMock: vi.fn(),
  loadBuildHistoryMock: vi.fn(),
  loadLocalizationSettingsMock: vi.fn(),
  openHostPathMock: vi.fn(),
  purgeBuildExecutionRetentionMock: vi.fn(),
  readHostTextFileMock: vi.fn(),
  readRetainedLogArchiveEntryMock: vi.fn(),
}));

vi.mock("../services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
}));

vi.mock("../services/processDetail", () => ({
  deleteReleaseProcessOutputs: deleteReleaseProcessOutputsMock,
  loadArtifactInspection: loadArtifactInspectionMock,
  loadBuildExecutionReport: loadBuildExecutionReportMock,
  loadBuildHistory: loadBuildHistoryMock,
  openHostPath: openHostPathMock,
  purgeBuildExecutionRetention: purgeBuildExecutionRetentionMock,
  readHostTextFile: readHostTextFileMock,
  readRetainedLogArchiveEntry: readRetainedLogArchiveEntryMock,
}));

import { LocalizationProvider } from "../LocalizationProvider";

const COMPLETED_PROCESS: ProcessFeedRecord = {
  canceled_build_runs: 0,
  canceled_publish_runs: 0,
  created_at: "2026-05-19T00:00:00Z",
  current_step_detail: "Build and publish completed.",
  current_step_label: "Completed",
  current_step_status: "succeeded",
  display_status: "succeeded",
  engine_version: "6000.0.23f1",
  error_message: null,
  failed_build_runs: 0,
  failed_publish_runs: 0,
  finished_at: "2026-05-19T00:12:00Z",
  git_commit: "abc1234",
  git_tag: "v0.1.0",
  queued_build_runs: 0,
  queued_publish_runs: 0,
  release_run_id: 77,
  repository_engine_kind: "unity",
  repository_id: 1,
  repository_name: "Worker Demo",
  repository_url: "https://github.com/indiegabo/worker-demo.git",
  running_build_runs: 0,
  running_publish_runs: 0,
  started_at: "2026-05-19T00:00:10Z",
  succeeded_build_runs: 1,
  succeeded_publish_runs: 1,
  total_build_runs: 1,
  total_publish_runs: 1,
  updated_at: "2026-05-19T00:12:00Z",
};

const ON_HOLD_PROCESS: ProcessFeedRecord = {
  ...COMPLETED_PROCESS,
  current_step_detail:
    "Process on hold because Unity Editor appears to be open for the local workspace.",
  current_step_label: "Awaiting Unity editor lock release",
  current_step_status: "on_hold",
  display_status: "on_hold",
  finished_at: null,
  running_build_runs: 1,
  succeeded_build_runs: 0,
  succeeded_publish_runs: 0,
  updated_at: "2026-05-19T00:03:00Z",
};

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
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
            "process_detail.actions.cancel_requested":
              "Cancel request accepted. The process feed will refresh as soon as the runtime snapshot advances.",
            "process_detail.confirm.cancel.cancel": "Keep process running",
            "process_detail.confirm.cancel.confirm": "Cancel process",
            "process_detail.confirm.cancel.description":
              "This cancels the active process and marks the in-flight build as canceled.",
            "process_detail.confirm.cancel.message":
              "Use this when you do not want to close Unity right now. To continue this run, close Unity Editor and HGP will resume from the on-hold gate.",
            "process_detail.confirm.cancel.title": "Cancel process?",
            "process_detail.current_step.actions.cancel": "Cancel process",
            "process_detail.current_step.on_hold.guidance":
              "Close Unity Editor to continue this process. HGP blocks this step intentionally to keep automation consistent, because changing files while a local snapshot is being prepared can invalidate build inputs.",
            "process_detail.final_outcome.title": "Final Outcome",
            "process_detail.execution_report.title": "Execution Report",
            "process_detail.execution_report.actions.view_json":
              "View JSON report",
            "process_feed.status.on_hold": "On hold",
            "process_detail.retained_logs.title": "Retained Logs",
            "process_detail.runtime_metadata.title": "Runtime Metadata",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "process_detail.actions.cancel_requested":
              "Solicitação de cancelamento aceita. O feed de processos será atualizado assim que o snapshot do runtime avançar.",
            "process_detail.confirm.cancel.cancel":
              "Manter processo em execução",
            "process_detail.confirm.cancel.confirm": "Cancelar processo",
            "process_detail.confirm.cancel.description":
              "Isto cancela o processo ativo e marca a build em andamento como cancelada.",
            "process_detail.confirm.cancel.message":
              "Use isto quando você não quiser fechar a Unity agora. Para continuar esta execução, feche o Unity Editor e o HGP retomará a partir do gate de espera.",
            "process_detail.confirm.cancel.title": "Cancelar processo?",
            "process_detail.current_step.actions.cancel": "Cancelar processo",
            "process_detail.current_step.on_hold.guidance":
              "Feche o Unity Editor para continuar este processo. O HGP bloqueia esta etapa intencionalmente para manter a automação consistente, porque alterar arquivos enquanto um snapshot local está sendo preparado pode invalidar os insumos da build.",
            "process_detail.final_outcome.title": "Resultado Final",
            "process_detail.execution_report.title": "Relatório de Execução",
            "process_detail.execution_report.actions.view_json":
              "Ver relatório JSON",
            "process_feed.status.on_hold": "Em espera",
            "process_detail.retained_logs.title": "Logs Retidos",
            "process_detail.runtime_metadata.title": "Metadados do Runtime",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });

  loadBuildHistoryMock.mockResolvedValue([
    {
      artifact_count: 0,
      artifact_root_path: "C:/tmp/artifacts",
      build_run_id: 11,
      build_target_id: 5,
      build_target_name: "Windows Build",
      created_at: "2026-05-19T00:00:20Z",
      engine_version: "6000.0.23f1",
      error_message: null,
      finished_at: "2026-05-19T00:08:00Z",
      git_commit: "abc1234",
      git_tag: "v0.1.0",
      image_ref: null,
      log_path: "C:/tmp/workspace/build.log",
      publish_run_count: 1,
      release_run_id: 77,
      repository_id: 1,
      repository_name: "Worker Demo",
      repository_url: "https://github.com/indiegabo/worker-demo.git",
      runner_type: "host-native",
      started_at: "2026-05-19T00:00:10Z",
      status: "succeeded",
      unity_build_method: "Builder.PerformBuild",
      unity_target_platform: "StandaloneWindows64",
      updated_at: "2026-05-19T00:08:00Z",
      workspace_path: "C:/tmp/workspace",
    },
  ]);
  loadArtifactInspectionMock.mockResolvedValue([]);
  loadBuildExecutionReportMock.mockResolvedValue({
    build_run_id: 11,
    exists: true,
    log_entries: [
      {
        compressed_size_bytes: 320,
        entry_name: "Editor.log",
        entry_path: "logs/Editor.log",
        size_bytes: 1500,
      },
    ],
    logs_archive_exists: true,
    logs_archive_path: "C:/tmp/retained/execution-logs.zip",
    report: {
      status: "ok",
      summary: {
        builds: 1,
        publishes: 1,
      },
    },
    report_path: "C:/tmp/retained/report.json",
    retained_dir_path: "C:/tmp/retained",
    workspace_path: "C:/tmp/workspace",
  });
  readRetainedLogArchiveEntryMock.mockResolvedValue({
    archive_path: "C:/tmp/retained/execution-logs.zip",
    content: "Build completed successfully.",
    entry_path: "logs/Editor.log",
    exists: true,
    size_bytes: 1500,
    truncated: false,
  });
  deleteReleaseProcessOutputsMock.mockResolvedValue({
    artifact_root_path: "C:/tmp/artifacts",
    missing_paths: [],
    release_run_id: 77,
    removed_paths: [],
  });
  purgeBuildExecutionRetentionMock.mockResolvedValue({
    build_run_id: 11,
    removed_paths: [],
    retained_dir_path: "C:/tmp/retained",
    workspace_path: "C:/tmp/workspace",
    workspace_removed: false,
  });
  openHostPathMock.mockResolvedValue(undefined);
});

describe("ProcessDetailFocusScreen localization", () => {
  it("renders translated process-detail chrome from the locale pack", async () => {
    render(
      <LocalizationProvider>
        <OverlayProvider>
          <ProcessDetailFocusScreen
            process={COMPLETED_PROCESS}
            usesLiveSnapshot
          />
        </OverlayProvider>
      </LocalizationProvider>,
    );

    expect(await screen.findByText("Resultado Final")).toBeInTheDocument();
    expect(screen.getByText("Relatório de Execução")).toBeInTheDocument();
    expect(screen.getByText("Logs Retidos")).toBeInTheDocument();
    expect(screen.getByText("Metadados do Runtime")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Ver relatório JSON" }),
    ).toBeInTheDocument();
  });

  it("renders localized on-hold guidance and cancel confirmation flow", async () => {
    const requestCancelMock = vi.fn().mockResolvedValue(undefined);

    render(
      <LocalizationProvider>
        <OverlayProvider>
          <ProcessDetailFocusScreen
            onRequestCancel={requestCancelMock}
            process={ON_HOLD_PROCESS}
            usesLiveSnapshot
          />
        </OverlayProvider>
      </LocalizationProvider>,
    );

    expect((await screen.findAllByText("Em espera")).length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        "Feche o Unity Editor para continuar este processo. O HGP bloqueia esta etapa intencionalmente para manter a automação consistente, porque alterar arquivos enquanto um snapshot local está sendo preparado pode invalidar os insumos da build.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancelar processo" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Cancelar processo?",
    });

    expect(
      within(dialog).getByText(
        "Isto cancela o processo ativo e marca a build em andamento como cancelada.",
      ),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        "Use isto quando você não quiser fechar a Unity agora. Para continuar esta execução, feche o Unity Editor e o HGP retomará a partir do gate de espera.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Cancelar processo" }),
    );

    await waitFor(() => {
      expect(requestCancelMock).toHaveBeenCalledWith(ON_HOLD_PROCESS);
    });
    expect(
      await screen.findByText(
        "Solicitação de cancelamento aceita. O feed de processos será atualizado assim que o snapshot do runtime avançar.",
      ),
    ).toBeInTheDocument();
  });
});

function buildHostTextFilePayload(path: string, content: string) {
  return {
    content,
    exists: true,
    path,
    size_bytes: content.length,
    truncated: false,
  };
}
