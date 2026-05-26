import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import FullScreenModal from "./FullScreenModal";

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
  { label: "Select a Unity target", value: "" },
  { label: "Windows", value: "StandaloneWindows64" },
  { label: "Linux", value: "StandaloneLinux64" },
  { label: "macOS", value: "StandaloneOSX" },
  { label: "WebGL", value: "WebGL" },
  { label: "Android", value: "Android" },
] as const;

const DEFAULT_CUSTOM_TARGET_PLATFORM = "StandaloneWindows64";

export function BuildTargetEditorOverlay({
  initialErrors = {},
  initialTarget,
  mode,
  onResolve,
  targetId,
}: BuildTargetEditorOverlayProps) {
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

  const fieldErrors = attemptedSave
    ? validateBuildTargetDraftForOverlay(draft, {
        isCustomConfigurationEnabled,
        suggestedBuildMethod,
      })
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
    });

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
          ? "Configure one build target and return once the target contract is ready."
          : "Update this build target and return once the target contract is ready."
      }
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
            Cancel
          </Button>
          <Button
            leadingIcon="plus"
            onClick={handleSave}
            size="sm"
            variant="primary"
          >
            {isCreateMode ? "Confirm" : "Save target"}
          </Button>
        </div>
      }
      onResolve={onResolve}
      title={isCreateMode ? "Add build target" : "Edit build target"}
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
              ? "Default configuration"
              : "Custom configuration"}
          </Button>
        </div>

        {!isCustomConfigurationEnabled ? (
          <>
            <SelectField
              data-overlay-autofocus
              error={fieldErrors.targetPlatform}
              hint="This writes the Unity targetPlatform contract field directly."
              label="Unity target platform"
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
              options={UNITY_TARGET_OPTIONS}
              value={normalizedTargetPlatform}
            />

            <div className="wizard-callout wizard-callout--compact">
              <p className="wizard-callout__title">Platform defaults</p>
              <p className="wizard-callout__copy">
                HGP derives the target name and Unity build method from the
                selected target platform by default. You still need to implement
                the static method in your Unity project.
              </p>
              <p className="wizard-callout__copy wizard-summary-list__copy--muted">
                Default target name:{" "}
                {resolveUnityBuildTargetName(normalizedTargetPlatform)}
              </p>
              <p className="wizard-callout__copy wizard-summary-list__copy--muted">
                Default build method:{" "}
                {suggestedBuildMethod ?? "Select a platform first"}
              </p>
            </div>
          </>
        ) : null}

        {isCustomConfigurationEnabled ? (
          <>
            <TextField
              data-overlay-autofocus
              error={fieldErrors.name}
              hint="Keep the custom target name stable. It becomes part of the artifact file name."
              label="Custom target name"
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
              hint="Use this only when your Unity project requires a non-standard method path for this custom target."
              label="Custom build method"
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
): BuildTargetEditorFieldErrors {
  const errors: BuildTargetEditorFieldErrors = {};

  if (!target.targetPlatform.trim()) {
    errors.targetPlatform = "Unity target platform is required.";
  }

  if (options.isCustomConfigurationEnabled) {
    if (!target.name.trim()) {
      errors.name = "Custom target name is required.";
    }

    if (!target.buildMethod.trim()) {
      errors.buildMethod = "Custom build method is required.";
    } else if (!target.buildMethod.includes(".")) {
      errors.buildMethod =
        "Use a full static method path such as Builder.PerformWindows.";
    }
  } else if (!options.suggestedBuildMethod) {
    errors.buildMethod =
      "Select a supported Unity target platform or enable method override.";
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

  return option?.label || normalizedTargetPlatform || "";
}
