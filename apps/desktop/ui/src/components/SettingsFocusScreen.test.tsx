import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  loadLocalizationSettingsMock,
  saveLocalizationPreferencesMock,
} = vi.hoisted(() => ({
  loadLocalizationSettingsMock: vi.fn(),
  saveLocalizationPreferencesMock: vi.fn(),
}));

vi.mock("../services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
  saveLocalizationPreferences: saveLocalizationPreferencesMock,
}));

import { SettingsFocusScreen } from "./SettingsFocusScreen";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadLocalizationSettingsMock.mockResolvedValue(buildLocalizationSettings());
  saveLocalizationPreferencesMock.mockResolvedValue(
    buildLocalizationSettings(),
  );
});

describe("SettingsFocusScreen", () => {
  it("renders a compact localization-only surface", async () => {
    render(<SettingsFocusScreen />);

    expect(
      await screen.findByRole("heading", { name: "Settings" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", {
        name: "Idioma",
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Credential Inventory" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Shell And Runtime" }),
    ).not.toBeInTheDocument();

    expect(
      screen.queryByRole("button", { name: "Open localization root" }),
    ).not.toBeInTheDocument();
  });

  it("saves shell localization preferences from the settings screen", async () => {
    saveLocalizationPreferencesMock.mockResolvedValueOnce(
      buildLocalizationSettings({
        fallback_locale: "pt-BR",
        primary_locale: "pt-BR",
      }),
    );

    render(<SettingsFocusScreen />);

    await screen.findByRole("heading", { name: "Settings" });
    const primaryLanguageField = await screen.findByLabelText("Idioma");

    fireEvent.change(primaryLanguageField, {
      target: { value: "pt-BR" },
    });

    await waitFor(() => {
      expect(saveLocalizationPreferencesMock).toHaveBeenCalledWith({
        fallback_locale: "pt-BR",
        primary_locale: "pt-BR",
      });
    });

    expect(
      await screen.findByText("Idioma salvo."),
    ).toBeInTheDocument();
    expect(primaryLanguageField).toHaveValue("pt-BR");
  });
});

function buildLocalizationSettings(
  overrides?: Partial<{
    available_locales: Array<{
      code: string;
      display_name: string;
      is_official: boolean;
      message_count: number;
      native_name: string;
    }>;
    fallback_locale: string;
    localization_root: string;
    primary_locale: string;
    warnings: string[];
  }>,
) {
  return {
    available_locales: [
      {
        code: "en",
        display_name: "English",
        is_official: true,
        message_count: 3,
        native_name: "English",
      },
      {
        code: "pt-BR",
        display_name: "Brazilian Portuguese",
        is_official: true,
        message_count: 3,
        native_name: "Português (Brasil)",
      },
    ],
    fallback_locale: "pt-BR",
    localization_root:
      "C:/Users/indie/projetos/Apps/handy-unity-publisher/apps/desktop/src-tauri/localizations",
    primary_locale: "en",
    warnings: [],
    ...overrides,
  };
}
