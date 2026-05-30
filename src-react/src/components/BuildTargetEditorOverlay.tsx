import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField, type SelectOption } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { useLocalization, type Translate } from "../LocalizationProvider";

export type SharedBuildTargetDraft = {
  id: string;
  name: string;
  targetPlatform: string;
  buildMethod: string;
};

export type BuildTargetEditorOverlayMode = "create" | "edit";

export type BuildTargetEditorFieldErrors = {
  name?: string;
  targetPlatform?: string;
  buildMethod?: string;
};

export type BuildTargetEditorOverlayResult = {
  target: SharedBuildTargetDraft;
};

type ExistingBuildTargetPlatform = Pick<
  SharedBuildTargetDraft,
  "id" | "targetPlatform"
>;

type BuildTargetEditorOverlayProps = {
  existingTargets?: readonly ExistingBuildTargetPlatform[];
  initialErrors?: BuildTargetEditorFieldErrors;
  initialTarget: SharedBuildTargetDraft;
  mode: BuildTargetEditorOverlayMode;
  onResolve?: (value?: BuildTargetEditorOverlayResult | null) => void;
  targetId: string;
};

type UnityTargetCatalogEntry = {
  aliases?: readonly string[];
  buildMethod: string;
  fallbackLabel: string;
  group: UnityTargetGroupId;
  key: string;
  targetName: string;
  value: string;
};

type UnityTargetGroupId =
  | "desktop"
  | "mobileAndXr"
  | "webAndStore"
  | "consoles"
  | "servers";

const UNITY_TARGET_PLACEHOLDER = {
  fallbackLabel: "Select a Unity target",
  key: "build_target_editor.target_platform.placeholder",
  value: "",
} as const;

const UNITY_TARGET_GROUP_LABELS: Record<UnityTargetGroupId, string> = {
  consoles: "Consoles",
  desktop: "Desktop",
  mobileAndXr: "Mobile and XR",
  servers: "Servers",
  webAndStore: "Web and Store",
};

const UNITY_TARGET_GROUP_ORDER: readonly UnityTargetGroupId[] = [
  "desktop",
  "mobileAndXr",
  "webAndStore",
  "consoles",
  "servers",
];

const UNITY_TARGET_CATALOG: readonly UnityTargetCatalogEntry[] = [
  {
    aliases: ["windows32", "windows 32-bit", "windows x86"],
    buildMethod: "Builder.PerformWindows32",
    fallbackLabel: "Windows 32-bit",
    group: "desktop",
    key: "build_target_editor.target_platform.windows_32",
    targetName: "Windows 32-bit",
    value: "StandaloneWindows",
  },
  {
    aliases: ["windows", "windows64", "windows 64-bit"],
    buildMethod: "Builder.PerformWindows64",
    fallbackLabel: "Windows 64-bit",
    group: "desktop",
    key: "build_target_editor.target_platform.windows_64",
    targetName: "Windows 64-bit",
    value: "StandaloneWindows64",
  },
  {
    aliases: ["mac", "macos", "osx"],
    buildMethod: "Builder.PerformMacOS",
    fallbackLabel: "macOS",
    group: "desktop",
    key: "build_target_editor.target_platform.macos",
    targetName: "macOS",
    value: "StandaloneOSX",
  },
  {
    aliases: ["linux", "linux64", "linux 64-bit"],
    buildMethod: "Builder.PerformLinux64",
    fallbackLabel: "Linux 64-bit",
    group: "desktop",
    key: "build_target_editor.target_platform.linux_64",
    targetName: "Linux 64-bit",
    value: "StandaloneLinux64",
  },
  {
    aliases: ["ios"],
    buildMethod: "Builder.PerformIOS",
    fallbackLabel: "iOS",
    group: "mobileAndXr",
    key: "build_target_editor.target_platform.ios",
    targetName: "iOS",
    value: "iOS",
  },
  {
    aliases: ["android"],
    buildMethod: "Builder.PerformAndroid",
    fallbackLabel: "Android",
    group: "mobileAndXr",
    key: "build_target_editor.target_platform.android",
    targetName: "Android",
    value: "Android",
  },
  {
    aliases: ["tvos"],
    buildMethod: "Builder.PerformTvOS",
    fallbackLabel: "tvOS",
    group: "mobileAndXr",
    key: "build_target_editor.target_platform.tvos",
    targetName: "tvOS",
    value: "tvOS",
  },
  {
    aliases: ["vision os", "visionos"],
    buildMethod: "Builder.PerformVisionOS",
    fallbackLabel: "visionOS",
    group: "mobileAndXr",
    key: "build_target_editor.target_platform.visionos",
    targetName: "visionOS",
    value: "VisionOS",
  },
  {
    aliases: ["webgl"],
    buildMethod: "Builder.PerformWebGL",
    fallbackLabel: "WebGL",
    group: "webAndStore",
    key: "build_target_editor.target_platform.webgl",
    targetName: "WebGL",
    value: "WebGL",
  },
  {
    aliases: ["uwp", "wsa", "wsaplayer"],
    buildMethod: "Builder.PerformUWP",
    fallbackLabel: "UWP",
    group: "webAndStore",
    key: "build_target_editor.target_platform.uwp",
    targetName: "UWP",
    value: "WSAPlayer",
  },
  {
    aliases: ["ps4"],
    buildMethod: "Builder.PerformPS4",
    fallbackLabel: "PS4",
    group: "consoles",
    key: "build_target_editor.target_platform.ps4",
    targetName: "PS4",
    value: "PS4",
  },
  {
    aliases: ["ps5"],
    buildMethod: "Builder.PerformPS5",
    fallbackLabel: "PS5",
    group: "consoles",
    key: "build_target_editor.target_platform.ps5",
    targetName: "PS5",
    value: "PS5",
  },
  {
    aliases: ["xbox one", "xboxone"],
    buildMethod: "Builder.PerformXboxOne",
    fallbackLabel: "Xbox One",
    group: "consoles",
    key: "build_target_editor.target_platform.xbox_one",
    targetName: "Xbox One",
    value: "XboxOne",
  },
  {
    aliases: ["gamecore xbox one", "gamecorexboxone"],
    buildMethod: "Builder.PerformGameCoreXboxOne",
    fallbackLabel: "GameCore Xbox One",
    group: "consoles",
    key: "build_target_editor.target_platform.gamecore_xbox_one",
    targetName: "GameCore Xbox One",
    value: "GameCoreXboxOne",
  },
  {
    aliases: ["gamecore xbox series", "gamecorexboxseries", "xbox series"],
    buildMethod: "Builder.PerformGameCoreXboxSeries",
    fallbackLabel: "GameCore Xbox Series",
    group: "consoles",
    key: "build_target_editor.target_platform.gamecore_xbox_series",
    targetName: "GameCore Xbox Series",
    value: "GameCoreXboxSeries",
  },
  {
    aliases: ["nintendo switch", "switch"],
    buildMethod: "Builder.PerformSwitch",
    fallbackLabel: "Nintendo Switch",
    group: "consoles",
    key: "build_target_editor.target_platform.switch",
    targetName: "Nintendo Switch",
    value: "Switch",
  },
  {
    aliases: [
      "dedicated server linux",
      "dedicatedserverlinux",
      "linux dedicated server",
      "linuxheadlesssimulation",
    ],
    buildMethod: "Builder.PerformDedicatedServerLinux",
    fallbackLabel: "Dedicated Server Linux",
    group: "servers",
    key: "build_target_editor.target_platform.dedicated_server_linux",
    targetName: "Dedicated Server Linux",
    value: "LinuxHeadlessSimulation",
  },
] as const;

const DEFAULT_CUSTOM_TARGET_PLATFORM = "StandaloneWindows64";

export function BuildTargetEditorOverlay({
  existingTargets = [],
  initialErrors = {},
  initialTarget,
  mode,
  onResolve,
  targetId,
}: BuildTargetEditorOverlayProps) {
  const { t } = useLocalization();
  const isCreateMode = mode === "create";
  const initialNormalizedPlatform = normalizeUnityTargetPlatformValue(
    initialTarget.targetPlatform,
  );
  const initialSuggestedMethod = resolveSuggestedUnityBuildMethod(
    initialNormalizedPlatform,
  );
  const initialSuggestedName = resolveUnityBuildTargetName(
    initialNormalizedPlatform,
  );
  const [draft, setDraft] = useState<SharedBuildTargetDraft>(() => ({
    ...initialTarget,
    id: targetId,
  }));
  const [isCustomConfigurationEnabled, setIsCustomConfigurationEnabled] =
    useState(() => {
      if (isCreateMode) {
        return false;
      }

      const normalizedCurrentMethod = initialTarget.buildMethod.trim();
      const normalizedCurrentName = initialTarget.name.trim();

      return (
        normalizedCurrentMethod !== (initialSuggestedMethod ?? "") ||
        normalizedCurrentName !== initialSuggestedName
      );
    });
  const [attemptedSave, setAttemptedSave] = useState(false);
  const unavailableTargetPlatforms = buildUnavailableTargetPlatformSet(
    existingTargets,
    targetId,
  );

  const normalizedTargetPlatform = normalizeUnityTargetPlatformValue(
    draft.targetPlatform,
  );
  const suggestedBuildMethod = resolveSuggestedUnityBuildMethod(
    normalizedTargetPlatform,
  );
  const unityTargetOptions = buildUnityTargetOptions(
    t,
    unavailableTargetPlatforms,
  );

  const fieldErrors = attemptedSave
    ? validateBuildTargetDraftForOverlay(
        draft,
        {
          isCustomConfigurationEnabled,
          suggestedBuildMethod,
          unavailableTargetPlatforms,
        },
        t,
      )
    : initialErrors;

  const enableCustomConfiguration = () => {
    setIsCustomConfigurationEnabled(true);
    setDraft((current) => {
      const fallbackPlatform = current.targetPlatform.trim()
        ? normalizeUnityTargetPlatformValue(current.targetPlatform)
        : resolveDefaultCustomTargetPlatform(unavailableTargetPlatforms);

      return {
        ...current,
        targetPlatform: fallbackPlatform,
        buildMethod:
          current.buildMethod.trim() ||
          resolveSuggestedUnityBuildMethod(fallbackPlatform) ||
          "",
        name:
          current.name.trim() || resolveUnityBuildTargetName(fallbackPlatform),
      };
    });
  };

  const disableCustomConfiguration = () => {
    setIsCustomConfigurationEnabled(false);
    setDraft((current) => {
      const normalizedPlatform = normalizeUnityTargetPlatformValue(
        current.targetPlatform,
      );

      return {
        ...current,
        buildMethod: resolveSuggestedUnityBuildMethod(normalizedPlatform) ?? "",
        name: resolveUnityBuildTargetName(normalizedPlatform),
      };
    });
  };

  const handleSave = () => {
    setAttemptedSave(true);

    const errors = validateBuildTargetDraftForOverlay(
      draft,
      {
        isCustomConfigurationEnabled,
        suggestedBuildMethod,
        unavailableTargetPlatforms,
      },
      t,
    );

    if (firstBuildTargetFieldError(errors)) {
      return;
    }

    onResolve?.({
      target: {
        ...draft,
        buildMethod: isCustomConfigurationEnabled
          ? draft.buildMethod.trim()
          : (suggestedBuildMethod ?? ""),
        name: isCustomConfigurationEnabled
          ? draft.name.trim()
          : resolveUnityBuildTargetName(normalizedTargetPlatform),
        targetPlatform: normalizedTargetPlatform,
      },
    });
  };

  return (
    <FullScreenModal
      description={
        isCreateMode
          ? t(
              "build_target_editor.create.description",
              "Configure one build target and return once the target contract is ready.",
            )
          : t(
              "build_target_editor.edit.description",
              "Update this build target and return once the target contract is ready.",
            )
      }
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
            {t("build_target_editor.actions.cancel", "Cancel")}
          </Button>
          <Button
            leadingIcon="plus"
            onClick={handleSave}
            size="sm"
            variant="primary"
          >
            {isCreateMode
              ? t("build_target_editor.actions.confirm", "Confirm")
              : t("build_target_editor.actions.save", "Save target")}
          </Button>
        </div>
      }
      onResolve={onResolve}
      title={
        isCreateMode
          ? t("build_target_editor.create.title", "Add build target")
          : t("build_target_editor.edit.title", "Edit build target")
      }
    >
      <div className="project-detail-form-grid publish-destination-editor-modal__content">
        <div className="build-target-editor__mode-actions">
          <Button
            onClick={() => {
              if (isCustomConfigurationEnabled) {
                disableCustomConfiguration();
                return;
              }

              enableCustomConfiguration();
            }}
            size="sm"
            variant={isCustomConfigurationEnabled ? "ghost" : "secondary"}
          >
            {isCustomConfigurationEnabled
              ? t(
                  "build_target_editor.actions.default_configuration",
                  "Default configuration",
                )
              : t(
                  "build_target_editor.actions.custom_configuration",
                  "Custom configuration",
                )}
          </Button>
        </div>

        {!isCustomConfigurationEnabled ? (
          <>
            <SelectField
              data-overlay-autofocus
              error={fieldErrors.targetPlatform}
              hint={t(
                "build_target_editor.target_platform.hint",
                "This writes the Unity targetPlatform contract field directly.",
              )}
              label={t(
                "build_target_editor.target_platform.label",
                "Unity target platform",
              )}
              onChange={(event) => {
                const nextTargetPlatform = normalizeUnityTargetPlatformValue(
                  event.currentTarget.value,
                );
                setDraft((current) => ({
                  ...current,
                  targetPlatform: nextTargetPlatform,
                  buildMethod:
                    resolveSuggestedUnityBuildMethod(nextTargetPlatform) ?? "",
                  name: resolveUnityBuildTargetName(nextTargetPlatform),
                }));
              }}
              options={unityTargetOptions}
              value={normalizedTargetPlatform}
            />

            <div className="wizard-callout wizard-callout--compact">
              <p className="wizard-callout__title">
                {t("build_target_editor.defaults.title", "Platform defaults")}
              </p>
              <p className="wizard-callout__copy">
                {t(
                  "build_target_editor.defaults.copy",
                  "HGP derives the target name and Unity build method from the selected target platform by default. You still need to implement the static method in your Unity project.",
                )}
              </p>
              <p className="wizard-callout__copy wizard-summary-list__copy--muted">
                {t(
                  "build_target_editor.defaults.target_name",
                  "Default target name:",
                )}{" "}
                {resolveUnityBuildTargetName(normalizedTargetPlatform)}
              </p>
              <p className="wizard-callout__copy wizard-summary-list__copy--muted">
                {t(
                  "build_target_editor.defaults.build_method",
                  "Default build method:",
                )}{" "}
                {suggestedBuildMethod ??
                  t(
                    "build_target_editor.defaults.select_platform_first",
                    "Select a platform first",
                  )}
              </p>
            </div>
          </>
        ) : null}

        {isCustomConfigurationEnabled ? (
          <>
            <TextField
              data-overlay-autofocus
              error={fieldErrors.name}
              hint={t(
                "build_target_editor.custom_name.hint",
                "Keep the custom target name stable. It becomes part of the artifact file name.",
              )}
              label={t(
                "build_target_editor.custom_name.label",
                "Custom target name",
              )}
              onChange={(event) => {
                const nextName = event.currentTarget.value;
                setDraft((current) => ({ ...current, name: nextName }));
              }}
              placeholder={resolveUnityBuildTargetName(
                normalizedTargetPlatform,
              )}
              value={draft.name}
            />
            <TextField
              error={fieldErrors.buildMethod}
              hint={t(
                "build_target_editor.custom_method.hint",
                "Use this only when your Unity project requires a non-standard method path for this custom target.",
              )}
              label={t(
                "build_target_editor.custom_method.label",
                "Custom build method",
              )}
              onChange={(event) => {
                const nextBuildMethod = event.currentTarget.value;
                setDraft((current) => ({
                  ...current,
                  buildMethod: nextBuildMethod,
                }));
              }}
              placeholder={suggestedBuildMethod ?? "Builder.PerformWindows64"}
              value={draft.buildMethod}
            />
          </>
        ) : null}
      </div>
    </FullScreenModal>
  );
}

function validateBuildTargetDraftForOverlay(
  target: SharedBuildTargetDraft,
  options: {
    isCustomConfigurationEnabled: boolean;
    suggestedBuildMethod: string | null;
    unavailableTargetPlatforms: ReadonlySet<string>;
  },
  t: Translate,
): BuildTargetEditorFieldErrors {
  const errors: BuildTargetEditorFieldErrors = {};
  const normalizedTargetPlatform = normalizeUnityTargetPlatformValue(
    target.targetPlatform,
  );

  if (!normalizedTargetPlatform) {
    errors.targetPlatform = t(
      "build_target_editor.validation.target_platform_required",
      "Unity target platform is required.",
    );
  } else if (options.unavailableTargetPlatforms.has(normalizedTargetPlatform)) {
    errors.targetPlatform = t(
      "build_target_editor.validation.target_platform_duplicate",
      "This Unity target platform has already been added.",
    );
  }

  if (options.isCustomConfigurationEnabled) {
    if (!target.name.trim()) {
      errors.name = t(
        "build_target_editor.validation.custom_name_required",
        "Custom target name is required.",
      );
    }

    if (!target.buildMethod.trim()) {
      errors.buildMethod = t(
        "build_target_editor.validation.custom_method_required",
        "Custom build method is required.",
      );
    } else if (!target.buildMethod.includes(".")) {
      errors.buildMethod = t(
        "build_target_editor.validation.custom_method_format",
        "Use a full static method path such as Builder.PerformWindows64.",
      );
    }
  } else if (!options.suggestedBuildMethod) {
    errors.buildMethod = t(
      "build_target_editor.validation.build_method_unavailable",
      "Select a supported Unity target platform or enable method override.",
    );
  }

  return errors;
}

function firstBuildTargetFieldError(errors: BuildTargetEditorFieldErrors) {
  return errors.name ?? errors.targetPlatform ?? errors.buildMethod ?? null;
}

function normalizeUnityTargetPlatformValue(value: string) {
  return findUnityTargetEntry(value)?.value ?? value.trim();
}

function resolveSuggestedUnityBuildMethod(targetPlatform: string) {
  return findUnityTargetEntry(targetPlatform)?.buildMethod ?? null;
}

function resolveUnityBuildTargetName(targetPlatform: string) {
  const normalizedTargetPlatform =
    normalizeUnityTargetPlatformValue(targetPlatform);
  const option = findUnityTargetEntry(normalizedTargetPlatform);

  return option?.targetName || normalizedTargetPlatform || "";
}

function buildUnityTargetOptions(
  t: Translate,
  unavailableTargetPlatforms: ReadonlySet<string>,
) {
  const groupedOptions = UNITY_TARGET_GROUP_ORDER.map((groupId) => ({
    label: UNITY_TARGET_GROUP_LABELS[groupId],
    options: UNITY_TARGET_CATALOG.filter(
      (entry) => entry.group === groupId,
    ).map((entry) =>
      buildUnityTargetOption(
        entry,
        unavailableTargetPlatforms.has(entry.value),
      ),
    ),
  })).filter((group) => group.options.length > 0);

  return [
    {
      label: t(
        UNITY_TARGET_PLACEHOLDER.key,
        UNITY_TARGET_PLACEHOLDER.fallbackLabel,
      ),
      value: UNITY_TARGET_PLACEHOLDER.value,
    },
    ...groupedOptions,
  ];
}

function findUnityTargetEntry(value: string) {
  const normalizedValue = value.trim().toLocaleLowerCase();

  if (!normalizedValue) {
    return null;
  }

  return (
    UNITY_TARGET_CATALOG.find(
      (entry) =>
        entry.value.toLocaleLowerCase() === normalizedValue ||
        entry.aliases?.includes(normalizedValue),
    ) ?? null
  );
}

function buildUnityTargetOption(
  entry: UnityTargetCatalogEntry,
  disabled: boolean,
): SelectOption {
  return {
    disabled,
    label: entry.fallbackLabel,
    value: entry.value,
  };
}

function buildUnavailableTargetPlatformSet(
  existingTargets: readonly ExistingBuildTargetPlatform[],
  currentTargetId: string,
): ReadonlySet<string> {
  const unavailableTargetPlatforms = new Set<string>();

  for (const target of existingTargets) {
    if (target.id === currentTargetId) {
      continue;
    }

    const normalizedTargetPlatform = normalizeUnityTargetPlatformValue(
      target.targetPlatform,
    );

    if (normalizedTargetPlatform) {
      unavailableTargetPlatforms.add(normalizedTargetPlatform);
    }
  }

  return unavailableTargetPlatforms;
}

function resolveDefaultCustomTargetPlatform(
  unavailableTargetPlatforms: ReadonlySet<string>,
) {
  if (!unavailableTargetPlatforms.has(DEFAULT_CUSTOM_TARGET_PLATFORM)) {
    return DEFAULT_CUSTOM_TARGET_PLATFORM;
  }

  return findFirstAvailableTargetPlatform(unavailableTargetPlatforms) ?? "";
}

function findFirstAvailableTargetPlatform(
  unavailableTargetPlatforms: ReadonlySet<string>,
) {
  for (const entry of UNITY_TARGET_CATALOG) {
    if (!unavailableTargetPlatforms.has(entry.value)) {
      return entry.value;
    }
  }

  return null;
}
