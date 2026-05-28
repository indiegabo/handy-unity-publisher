import {
  createContext,
  startTransition,
  useContext,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type PropsWithChildren,
} from "react";

import { readHostTextFile } from "./services/processDetail";
import {
  loadLocalizationSettings,
  type LocalizationSettings,
} from "./services/runtime";

export const LOCALIZATION_SETTINGS_CHANGED_EVENT =
  "hgp-localization-settings-changed";

type LocalizationPrimitive = number | string;

export type LocalizationVariables = Record<string, LocalizationPrimitive>;

export type Translate = (
  key: string,
  fallbackText: string,
  variables?: LocalizationVariables,
) => string;

type LocalizationContextValue = {
  fallbackLocale: string;
  primaryLocale: string;
  ready: boolean;
  t: Translate;
};

type LocalizationState = {
  fallbackLocale: string;
  messages: Record<string, string>;
  primaryLocale: string;
  ready: boolean;
};

const DEFAULT_LOCALE_CODE = "en";
const MAX_LOCALIZATION_FILE_BYTES = 512 * 1024;
const DEFAULT_LOCALIZATION_STATE: LocalizationState = {
  fallbackLocale: DEFAULT_LOCALE_CODE,
  messages: {},
  primaryLocale: DEFAULT_LOCALE_CODE,
  ready: true,
};

const LocalizationContext = createContext<LocalizationContextValue>({
  fallbackLocale: DEFAULT_LOCALE_CODE,
  primaryLocale: DEFAULT_LOCALE_CODE,
  ready: true,
  t: (_key, fallbackText, variables) =>
    interpolateMessage(fallbackText, variables),
});

export function LocalizationProvider({ children }: PropsWithChildren) {
  const [state, setState] = useState<LocalizationState>(
    DEFAULT_LOCALIZATION_STATE,
  );
  const latestRequestIdRef = useRef(0);

  const loadMessages = useEffectEvent(
    async (settingsOverride?: LocalizationSettings) => {
      const requestId = latestRequestIdRef.current + 1;
      latestRequestIdRef.current = requestId;

      try {
        const settings = settingsOverride ?? (await loadLocalizationSettings());
        const messages = await loadMergedMessages(settings);

        if (requestId !== latestRequestIdRef.current) {
          return;
        }

        startTransition(() => {
          setState({
            fallbackLocale: settings.fallback_locale,
            messages,
            primaryLocale: settings.primary_locale,
            ready: true,
          });
        });
      } catch (error) {
        console.error("failed to load shell localization messages", error);
      }
    },
  );

  useEffect(() => {
    void loadMessages();
  }, []);

  useEffect(() => {
    const handleLocalizationSettingsChanged = (event: Event) => {
      const detail = (event as CustomEvent<LocalizationSettings | undefined>)
        .detail;
      void loadMessages(detail);
    };

    window.addEventListener(
      LOCALIZATION_SETTINGS_CHANGED_EVENT,
      handleLocalizationSettingsChanged,
    );

    return () => {
      window.removeEventListener(
        LOCALIZATION_SETTINGS_CHANGED_EVENT,
        handleLocalizationSettingsChanged,
      );
    };
  }, []);

  return (
    <LocalizationContext.Provider
      value={{
        fallbackLocale: state.fallbackLocale,
        primaryLocale: state.primaryLocale,
        ready: state.ready,
        t: (key, fallbackText, variables) =>
          interpolateMessage(
            resolveLocalizedMessage(state.messages, key, fallbackText),
            variables,
          ),
      }}
    >
      {children}
    </LocalizationContext.Provider>
  );
}

export function useLocalization() {
  return useContext(LocalizationContext);
}

export function emitLocalizationSettingsChanged(
  settings: LocalizationSettings,
) {
  if (typeof window === "undefined") {
    return;
  }

  window.dispatchEvent(
    new CustomEvent<LocalizationSettings>(LOCALIZATION_SETTINGS_CHANGED_EVENT, {
      detail: settings,
    }),
  );
}

async function loadMergedMessages(
  settings: LocalizationSettings,
): Promise<Record<string, string>> {
  const englishMessages = await loadLocaleMessages(
    settings.localization_root,
    DEFAULT_LOCALE_CODE,
  );
  const fallbackMessages =
    settings.fallback_locale === DEFAULT_LOCALE_CODE
      ? englishMessages
      : await loadLocaleMessages(
          settings.localization_root,
          settings.fallback_locale,
        );
  const primaryMessages =
    settings.primary_locale === settings.fallback_locale
      ? fallbackMessages
      : settings.primary_locale === DEFAULT_LOCALE_CODE
        ? englishMessages
        : await loadLocaleMessages(
            settings.localization_root,
            settings.primary_locale,
          );

  return {
    ...englishMessages,
    ...fallbackMessages,
    ...primaryMessages,
  };
}

async function loadLocaleMessages(
  localizationRoot: string,
  localeCode: string,
): Promise<Record<string, string>> {
  try {
    const payload = await readHostTextFile(
      joinHostPath(localizationRoot, `${localeCode}.json`),
      MAX_LOCALIZATION_FILE_BYTES,
    );

    if (!payload.exists || !payload.content.trim()) {
      return {};
    }

    return parseLocalizedMessages(payload.content);
  } catch (error) {
    console.error(`failed to load locale pack ${localeCode}`, error);
    return {};
  }
}

function joinHostPath(rootPath: string, childPath: string) {
  const separator = rootPath.includes("\\") ? "\\" : "/";
  return `${rootPath.replace(/[\\/]+$/, "")}${separator}${childPath}`;
}

function parseLocalizedMessages(content: string): Record<string, string> {
  try {
    const parsed = JSON.parse(content);

    if (!isRecord(parsed) || !isRecord(parsed.messages)) {
      return {};
    }

    return Object.entries(parsed.messages).reduce<Record<string, string>>(
      (messages, [key, value]) => {
        if (typeof value === "string") {
          messages[key] = value;
        }

        return messages;
      },
      {},
    );
  } catch (error) {
    console.error("failed to parse locale pack document", error);
    return {};
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function resolveLocalizedMessage(
  messages: Record<string, string>,
  key: string,
  fallbackText: string,
) {
  const resolved = messages[key];
  return typeof resolved === "string" && resolved.trim()
    ? resolved
    : fallbackText;
}

function interpolateMessage(
  message: string,
  variables?: LocalizationVariables,
) {
  if (!variables) {
    return message;
  }

  return message.replace(/\{\{\s*([\w.]+)\s*\}\}/g, (match, key) => {
    const value = variables[key];
    return value === undefined ? match : String(value);
  });
}
