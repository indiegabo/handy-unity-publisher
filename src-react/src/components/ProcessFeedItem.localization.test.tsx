import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProcessFeedRecord } from "./processFeedPresentation";

const { loadLocalizationSettingsMock, readHostTextFileMock } = vi.hoisted(
  () => ({
    loadLocalizationSettingsMock: vi.fn(),
    readHostTextFileMock: vi.fn(),
  }),
);

vi.mock("../services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
}));

vi.mock("../services/processDetail", () => ({
  readHostTextFile: readHostTextFileMock,
}));

import { LocalizationProvider } from "../LocalizationProvider";
import { ProcessFeedItem } from "./ProcessFeedItem";

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
            "process_feed.badges.engine_kind": "engine: {{engineKind}}",
            "process_feed.badges.engine_pending": "Engine pending",
            "process_feed.count.build.one": "1 build",
            "process_feed.fallback_step.on_hold":
              "This process is on hold until the local workspace lock is released.",
            "process_feed.fallback_step.succeeded":
              "All recorded work for this process finished cleanly.",
            "process_feed.item.accordion.collapse":
              "Collapse process #{{releaseRunId}}",
            "process_feed.item.accordion.expand":
              "Expand process #{{releaseRunId}}",
            "process_feed.item.actions.cancel": "Cancel process",
            "process_feed.item.actions.open_detail":
              "Open process detail #{{releaseRunId}}",
            "process_feed.on_hold.reason":
              "On hold because Unity Editor is open for this local workspace. Close Unity to resume, or cancel this process.",
            "process_feed.status.on_hold": "On hold",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "process_feed.badges.engine_kind": "engine: {{engineKind}}",
            "process_feed.badges.engine_pending": "Engine pendente",
            "process_feed.count.build.one": "1 build",
            "process_feed.fallback_step.on_hold":
              "Este processo está em espera até que o lock do workspace local seja liberado.",
            "process_feed.fallback_step.succeeded":
              "Todo o trabalho registrado para este processo terminou com sucesso.",
            "process_feed.item.accordion.collapse":
              "Recolher processo #{{releaseRunId}}",
            "process_feed.item.accordion.expand":
              "Expandir processo #{{releaseRunId}}",
            "process_feed.item.actions.cancel": "Cancelar processo",
            "process_feed.item.actions.open_detail":
              "Abrir detalhe do processo #{{releaseRunId}}",
            "process_feed.on_hold.reason":
              "Em espera porque o Unity Editor está aberto para este workspace local. Feche o Unity para retomar ou cancele este processo.",
            "process_feed.status.on_hold": "Em espera",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });
});

describe("ProcessFeedItem localization", () => {
  it("renders translated feed-card chrome from the official locale pack", async () => {
    const onOpenDetail = vi.fn();

    render(
      <LocalizationProvider>
        <ProcessFeedItem onOpenDetail={onOpenDetail} process={PROCESS_RECORD} />
      </LocalizationProvider>,
    );

    expect(await screen.findByText("Engine pendente")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Todo o trabalho registrado para este processo terminou com sucesso.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Abrir detalhe do processo #77",
      }),
    );

    expect(
      screen.getByRole("button", { name: "Expandir processo #77" }),
    ).toBeInTheDocument();
    expect(onOpenDetail).toHaveBeenCalledWith(PROCESS_RECORD);
  });

  it("renders translated on-hold status and fallback guidance", async () => {
    const onRequestCancel = vi.fn();

    render(
      <LocalizationProvider>
        <ProcessFeedItem
          onOpenDetail={vi.fn()}
          onRequestCancel={onRequestCancel}
          process={{
            ...PROCESS_RECORD,
            current_step_status: "on_hold",
            display_status: "on_hold",
            current_step_detail: null,
          }}
        />
      </LocalizationProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "Cancelar processo" }),
    ).toBeInTheDocument();

    fireEvent.click(
      await screen.findByRole("button", { name: "Expandir processo #77" }),
    );

    expect(
      await screen.findByText(
        "Em espera porque o Unity Editor está aberto para este workspace local. Feche o Unity para retomar ou cancele este processo.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancelar processo" }));
    expect(onRequestCancel).toHaveBeenCalledTimes(1);
  });
});

const PROCESS_RECORD: ProcessFeedRecord = {
  canceled_build_runs: 0,
  canceled_publish_runs: 0,
  created_at: "2026-05-19T00:00:00Z",
  current_step_detail: null,
  current_step_label: "",
  current_step_status: "succeeded",
  display_status: "succeeded",
  engine_version: null,
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

function buildHostTextFilePayload(path: string, content: string) {
  return {
    content,
    exists: true,
    path,
    size_bytes: content.length,
    truncated: false,
  };
}
