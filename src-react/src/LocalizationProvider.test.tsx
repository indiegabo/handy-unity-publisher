import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { loadLocalizationSettingsMock, readHostTextFileMock } = vi.hoisted(
  () => ({
    loadLocalizationSettingsMock: vi.fn(),
    readHostTextFileMock: vi.fn(),
  }),
);

vi.mock("./services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
}));

vi.mock("./services/processDetail", () => ({
  readHostTextFile: readHostTextFileMock,
}));

import {
  LocalizationProvider,
  emitLocalizationSettingsChanged,
  useLocalization,
} from "./LocalizationProvider";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadLocalizationSettingsMock.mockResolvedValue(buildLocalizationSettings());
  readHostTextFileMock.mockImplementation(async (path: string) => {
    if (path.endsWith("en.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "app.main.navigation.projects": "Projects",
            "app.main.feed.empty.title": "No running processes",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "app.main.navigation.projects": "Projetos",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });
});

describe("LocalizationProvider", () => {
  it("merges english fallback messages with the selected primary locale", async () => {
    render(
      <LocalizationProvider>
        <LocalizationHarness />
      </LocalizationProvider>,
    );

    expect(await screen.findByText("Projetos")).toBeInTheDocument();
    expect(await screen.findByText("No running processes")).toBeInTheDocument();
  });

  it("reloads messages when the UI broadcasts updated localization settings", async () => {
    render(
      <LocalizationProvider>
        <LocalizationHarness />
      </LocalizationProvider>,
    );

    expect(await screen.findByText("Projetos")).toBeInTheDocument();

    readHostTextFileMock.mockImplementation(async (path: string) => {
      if (path.endsWith("en.json")) {
        return buildHostTextFilePayload(
          path,
          JSON.stringify({
            messages: {
              "app.main.navigation.projects": "Projects",
              "app.main.feed.empty.title": "No running processes",
            },
          }),
        );
      }

      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "app.main.navigation.projects": "Projects",
          },
        }),
      );
    });

    act(() => {
      emitLocalizationSettingsChanged(
        buildLocalizationSettings({
          fallback_locale: "en",
          primary_locale: "en",
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getAllByText("Projects").length).toBeGreaterThan(0);
    });
  });
});

function LocalizationHarness() {
  const { t } = useLocalization();

  return (
    <div>
      <p>{t("app.main.navigation.projects", "Projects")}</p>
      <p>{t("app.main.feed.empty.title", "No running processes")}</p>
    </div>
  );
}

function buildLocalizationSettings(overrides?: {
  fallback_locale?: string;
  primary_locale?: string;
}) {
  return {
    available_locales: [
      {
        code: "en",
        display_name: "English",
        is_official: true,
        message_count: 2,
        native_name: "English",
      },
      {
        code: "pt-BR",
        display_name: "Português (Brasil)",
        is_official: true,
        message_count: 1,
        native_name: "Português (Brasil)",
      },
    ],
    fallback_locale: overrides?.fallback_locale ?? "en",
    localization_root: "C:/hgp/localizations",
    primary_locale: overrides?.primary_locale ?? "pt-BR",
    warnings: [],
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
