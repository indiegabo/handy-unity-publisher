import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
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

type BuildTargetEditorOverlayProps = {
  initialErrors?: BuildTargetEditorFieldErrors;
  initialTarget: SharedBuildTargetDraft;
  mode: BuildTargetEditorOverlayMode;
  onResolve?: (value?: BuildTargetEditorOverlayResult | null) => void;
  targetId: string;
};

const UNITY_TARGET_OPTIONS = [
  {
    fallbackLabel: "Select a Unity target",
    key: "build_target_editor.target_platform.placeholder",
    targetName: "",
    value: "",
  },
  {
    fallbackLabel: "Windows",
    key: "build_target_editor.target_platform.windows",
    targetName: "Windows",
    value: "StandaloneWindows64",
  },
  {
    fallbackLabel: "Linux",
    key: "build_target_editor.target_platform.linux",
    targetName: "Linux",
    value: "StandaloneLinux64",
  },
  {
    fallbackLabel: "macOS",
    key: "build_target_editor.target_platform.macos",
    targetName: "macOS",
    value: "StandaloneOSX",
  },
  {
    fallbackLabel: "WebGL",
    key: "build_target_editor.target_platform.webgl",
    targetName: "WebGL",
    value: "WebGL",
  },
  {
    fallbackLabel: "Android",
    key: "build_target_editor.target_platform.android",
    targetName: "Android",
    value: "Android",
  },
] as const;

const DEFAULT_CUSTOM_TARGET_PLATFORM = "StandaloneWindows64";

export function BuildTargetEditorOverlay({
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

  const normalizedTargetPlatform = normalizeUnityTargetPlatformValue(
    draft.targetPlatform,
  );
  const suggestedBuildMethod = resolveSuggestedUnityBuildMethod(
    normalizedTargetPlatform,
  );
  const unityTargetOptions = buildUnityTargetOptions(t);

  const fieldErrors = attemptedSave
    ? validateBuildTargetDraftForOverlay(draft, {
        isCustomConfigurationEnabled,
        suggestedBuildMethod,
      }, t)
    : initialErrors;

  const enableCustomConfiguration = () => {
    setIsCustomConfigurationEnabled(true);
    setDraft((current) => {
      const fallbackPlatform = current.targetPlatform.trim()
        ? normalizeUnityTargetPlatformValue(current.targetPlatform)
        : DEFAULT_CUSTOM_TARGET_PLATFORM;

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

    const errors = validateBuildTargetDraftForOverlay(draft, {
      isCustomConfigurationEnabled,
      suggestedBuildMethod,
    }, t);

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
                {t(
                  "build_target_editor.defaults.title",
                  "Platform defaults",
                )}
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
              placeholder={suggestedBuildMethod ?? "Builder.PerformWindows"}
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
  },
  t: Translate,
): BuildTargetEditorFieldErrors {
  const errors: BuildTargetEditorFieldErrors = {};

  if (!target.targetPlatform.trim()) {
    errors.targetPlatform = t(
      "build_target_editor.validation.target_platform_required",
      "Unity target platform is required.",
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
        "Use a full static method path such as Builder.PerformWindows.",
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
  switch (value.trim().toLocaleLowerCase()) {
    case "windows":
      return "StandaloneWindows64";
    case "linux":
      return "StandaloneLinux64";
    case "macos":
    case "mac":
    case "osx":
      return "StandaloneOSX";
    case "webgl":
      return "WebGL";
    case "android":
      return "Android";
    default:
      return value.trim();
  }
}

function resolveSuggestedUnityBuildMethod(targetPlatform: string) {
  switch (targetPlatform.trim()) {
    case "StandaloneWindows64":
      return "Builder.PerformWindows";
    case "StandaloneLinux64":
      return "Builder.PerformLinux";
    case "StandaloneOSX":
      return "Builder.PerformMacOS";
    case "WebGL":
      return "Builder.PerformWebGL";
    case "Android":
      return "Builder.PerformAndroid";
    default:
      return null;
  }
}

function resolveUnityBuildTargetName(targetPlatform: string) {
  const normalizedTargetPlatform =
    normalizeUnityTargetPlatformValue(targetPlatform);
  const option = UNITY_TARGET_OPTIONS.find(
    (entry) => entry.value === normalizedTargetPlatform,
  );

  return option?.targetName || normalizedTargetPlatform || "";
}

function buildUnityTargetOptions(t: Translate) {
  return UNITY_TARGET_OPTIONS.map((option) => ({
    label: t(option.key, option.fallbackLabel),
    value: option.value,
  }));
}
