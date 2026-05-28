import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { SelectField } from "./Field";
import ScreenScaffold from "./ScreenScaffold";
import { SurfacePanel } from "./Surface";
import {
  emitLocalizationSettingsChanged,
  useLocalization,
} from "../LocalizationProvider";
import {
  loadLocalizationSettings,
  saveLocalizationPreferences,
  type LocalizationSettings as RuntimeLocalizationSettings,
  type SaveLocalizationPreferencesInput,
} from "../services/runtime";

type SettingsSnapshot = {
  isLoading: boolean;
  localizationSettings: RuntimeLocalizationSettings | null;
  localizationSettingsError: string | null;
};

const EMPTY_SETTINGS_SNAPSHOT: SettingsSnapshot = {
  isLoading: true,
  localizationSettings: null,
  localizationSettingsError: null,
};

export function SettingsFocusScreen() {
  const { t } = useLocalization();
  const [snapshot, setSnapshot] = useState<SettingsSnapshot>(
    EMPTY_SETTINGS_SNAPSHOT,
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [isSavingLocalization, setIsSavingLocalization] = useState(false);
  const latestRequestIdRef = useRef(0);

  const refreshSnapshot = useEffectEvent(async () => {
    const requestId = latestRequestIdRef.current + 1;
    latestRequestIdRef.current = requestId;

    startTransition(() => {
      setSnapshot((current) => ({
        ...current,
        isLoading: true,
      }));
    });

    try {
      const localizationSettings = await loadLocalizationSettings();

      if (requestId !== latestRequestIdRef.current) {
        return;
      }

      startTransition(() => {
        setSnapshot({
          isLoading: false,
          localizationSettings,
          localizationSettingsError: null,
        });
      });
    } catch (error) {
      if (requestId !== latestRequestIdRef.current) {
        return;
      }

      startTransition(() => {
        setSnapshot((current) => ({
          ...current,
          isLoading: false,
          localizationSettingsError: buildErrorMessage(t, error),
        }));
      });
    }
  });

  useEffect(() => {
    void refreshSnapshot();
  }, []);

  const handleSaveLocalizationPreferences = useEffectEvent(
    async (input: SaveLocalizationPreferencesInput) => {
      startTransition(() => {
        setActionError(null);
        setActionMessage(null);
        setIsSavingLocalization(true);
      });

      try {
        const localizationSettings = await saveLocalizationPreferences(input);
        emitLocalizationSettingsChanged(localizationSettings);
        startTransition(() => {
          setSnapshot((current) => ({
            ...current,
            localizationSettings,
            localizationSettingsError: null,
          }));
          setActionMessage(
            t("settings.localization.messages.saved", "Language saved."),
          );
        });
      } catch (error) {
        startTransition(() => {
          setActionError(
            t(
              "settings.localization.messages.save_failed",
              "Could not save language: {{message}}",
              {
                message: buildErrorMessage(t, error),
              },
            ),
          );
        });
      } finally {
        startTransition(() => {
          setIsSavingLocalization(false);
        });
      }
    },
  );

  const handlePrimaryLocaleChange = useEffectEvent(
    async (primaryLocale: string) => {
      if (!snapshot.localizationSettings) {
        return;
      }

      await handleSaveLocalizationPreferences({
        fallback_locale: snapshot.localizationSettings.fallback_locale,
        primary_locale: primaryLocale,
      });
    },
  );

  const handleFallbackLocaleChange = useEffectEvent(
    async (fallbackLocale: string) => {
      if (!snapshot.localizationSettings) {
        return;
      }

      await handleSaveLocalizationPreferences({
        fallback_locale: fallbackLocale,
        primary_locale: snapshot.localizationSettings.primary_locale,
      });
    },
  );

  const primaryLocalizationOptions = buildLocalizationOptions(
    snapshot.localizationSettings,
  );
  const fallbackLocalizationOptions = buildLocalizationOptions(
    snapshot.localizationSettings,
    true,
  );

  return (
    <div className="settings-focus-shell">
      <ScreenScaffold
        actions={
          <Button
            disabled={snapshot.isLoading}
            leadingIcon="refresh"
            onClick={() => void refreshSnapshot()}
            size="sm"
            variant="secondary"
          >
            {snapshot.isLoading
              ? t("settings.actions.refreshing", "Refreshing settings...")
              : t("settings.actions.refresh", "Refresh settings")}
          </Button>
        }
        title={t("settings.title", "Settings")}
      >
        {actionMessage ? (
          <p className="notice-banner">{actionMessage}</p>
        ) : null}
        {actionError ? (
          <p className="feed-banner feed-banner--error">{actionError}</p>
        ) : null}

        <SurfacePanel title={t("settings.localization.title", "Localization")}>
          {snapshot.localizationSettingsError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.localizationSettingsError}
            </p>
          ) : null}

          {snapshot.localizationSettings ? (
            <div className="settings-focus-panel-stack">
              <div className="settings-focus-form-grid">
                <SelectField
                  disabled={isSavingLocalization}
                  label={t(
                    "settings.localization.primary_label",
                    "Primary language",
                  )}
                  onChange={(event) =>
                    void handlePrimaryLocaleChange(event.currentTarget.value)
                  }
                  options={primaryLocalizationOptions}
                  value={snapshot.localizationSettings.primary_locale}
                />
                <SelectField
                  disabled={isSavingLocalization}
                  label={t(
                    "settings.localization.fallback_label",
                    "Fallback language",
                  )}
                  onChange={(event) =>
                    void handleFallbackLocaleChange(event.currentTarget.value)
                  }
                  options={fallbackLocalizationOptions}
                  value={snapshot.localizationSettings.fallback_locale}
                />
              </div>
            </div>
          ) : (
            <p className="settings-focus-copy">
              {t("settings.localization.loading", "Loading locales...")}
            </p>
          )}
        </SurfacePanel>
      </ScreenScaffold>
    </div>
  );
}

function buildLocalizationOptions(
  localizationSettings: RuntimeLocalizationSettings | null,
  officialOnly = false,
) {
  if (!localizationSettings) {
    return [];
  }

  return localizationSettings.available_locales
    .filter((locale) => !officialOnly || locale.is_official)
    .map((locale) => ({
      label: `${locale.native_name} (${locale.code})`,
      title: locale.display_name,
      value: locale.code,
    }));
}

function buildErrorMessage(
  t: ReturnType<typeof useLocalization>["t"],
  error: unknown,
) {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t("settings.error.unknown", "Unknown shell error.");
}
