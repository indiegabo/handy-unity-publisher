import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
import { BuildTargetRemovalCallout } from "./BuildTargetRemovalCallout";

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
            "project_shared.build_target_removal.actions.cancel": "Cancel",
            "project_shared.build_target_removal.actions.confirm":
              "Remove build target and bindings",
            "project_shared.build_target_removal.copy.with_bindings":
              "Removing {{targetName}} also removes publish bindings from {{bindingImpact}}.",
            "project_shared.build_target_removal.target_fallback":
              "this build target",
            "project_shared.build_target_removal.title":
              "Confirm build target removal",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "project_shared.build_target_removal.actions.cancel": "Cancelar",
            "project_shared.build_target_removal.actions.confirm":
              "Remover target de build e vínculos",
            "project_shared.build_target_removal.copy.with_bindings":
              "Remover {{targetName}} também remove vínculos de publicação de {{bindingImpact}}.",
            "project_shared.build_target_removal.target_fallback":
              "este target de build",
            "project_shared.build_target_removal.title":
              "Confirmar remoção do target de build",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });
});

describe("BuildTargetRemovalCallout localization", () => {
  it("renders translated removal-copy from the official locale pack", async () => {
    render(
      <LocalizationProvider>
        <BuildTargetRemovalCallout
          bindingImpact={["Itch Release"]}
          onCancel={() => undefined}
          onConfirm={() => undefined}
          targetName=""
        />
      </LocalizationProvider>,
    );

    expect(
      await screen.findByText("Confirmar remoção do target de build"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Remover este target de build também remove vínculos de publicação de Itch Release.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Remover target de build e vínculos",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Cancelar" }),
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
