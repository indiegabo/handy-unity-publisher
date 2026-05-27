import { type ReactNode } from "react";

import { Button } from "../Button";
import { SelectField, TextField, type SelectOption } from "../Field";
import { PathPickerField } from "../PathPickerField";
import {
  PublishDestinationsEditor,
  buildPublishDestinationReviewSummary,
  listUnboundBuildTargetNames,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
  type PublishDestinationValidationErrors,
} from "../PublishDestinationsEditor";
import { RepositoryEngineField } from "../RepositoryEngineField";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "../Surface";
import { type AuthProviderStatus } from "../../services/auth";
import {
  type DiscoveredUnityEditor,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryInspectionEntry,
  type RepositoryProviderDetection,
  type SaveSecretCredentialInput,
  type SecretCredentialSetting,
  type UnityExecutableValidation,
} from "../../services/projects";

export type BuildTargetDraft = {
  id: string;
  name: string;
  targetPlatform: string;
  buildMethod: string;
  buildTargetId?: number | null;
  unityExecutablePath?: string;
};

export type ProjectDraft = {
  projectKind: "repository" | "local";
  engineKind: RepositoryEngineKind;
  name: string;
  repositoryUrl: string;
  localPath: string;
  repositoryVisibility: "public" | "private";
  pollingIntervalSeconds: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  unityExecutablePath: string;
  buildTargets: BuildTargetDraft[];
  publishDestinations: PublishDestinationDraft[];
};

export type WizardStepKey =
  | "identity"
  | "access"
  | "targets"
  | "publish"
  | "paths"
  | "review";

export type WizardStepDefinition = {
  key: WizardStepKey;
  label: string;
  description: string;
};

export type ProjectSourceWizardAdapter = {
  kind: "repository" | "local";
  stepLabel: string;
  stepDescription: string;
  supportTitle: string;
  supportDescription: string;
  supportCopy: string;
  unsupportedMessage: string | null;
};

export type BuildTargetWizardAdapter = {
  kind: "unity" | "engine-unsupported";
  stepLabel: string;
  stepDescription: string;
  supportTitle: string;
  supportDescription: string;
  supportCopy: string;
  reviewDescription: string;
  unsupportedMessage: string | null;
};

export type TargetFieldErrors = {
  name?: string;
  targetPlatform?: string;
  buildMethod?: string;
};

export type TargetStepErrors = {
  root?: string;
  targets: Record<string, TargetFieldErrors>;
};

export type PathStepErrors = {
  artifactsRootOverride?: string;
  workspaceRootOverride?: string;
};

export const WIZARD_STEP_ORDER: readonly WizardStepKey[] = [
  "identity",
  "access",
  "targets",
  "publish",
  "paths",
  "review",
];

export const PROJECT_KIND_OPTIONS = [
  { label: "Repository project", value: "repository" },
  { label: "Local workspace project", value: "local" },
] as const;

export const REPOSITORY_VISIBILITY_OPTIONS = [
  { label: "Public", value: "public" },
  { label: "Private", value: "private" },
] as const;

export function formatProjectKindLabel(projectKind: ProjectDraft["projectKind"]) {
  return projectKind === "repository"
    ? "Repository project"
    : "Local workspace project";
}

export function formatRepositoryEngineKindLabel(
  engineKind: RepositoryEngineKind,
) {
  if (engineKind === "unity") {
    return "Unity";
  }

  return engineKind
    .split("-")
    .map((segment) =>
      segment.length > 0
        ? `${segment[0].toUpperCase()}${segment.slice(1)}`
        : segment,
    )
    .join(" ");
}

export function formatWizardTargetCount(targetCount: number) {
  return `${targetCount} target${targetCount === 1 ? "" : "s"}`;
}

export function formatProjectSourceAdapterStatus(
  adapter: ProjectSourceWizardAdapter,
) {
  return adapter.kind === "repository" || adapter.kind === "local"
    ? "Available"
    : "Unavailable";
}

export function resolveProjectSourceWizardAdapter(
  projectKind: ProjectDraft["projectKind"],
): ProjectSourceWizardAdapter {
  if (projectKind === "repository") {
    return {
      kind: "repository",
      stepLabel: "Repository",
      stepDescription:
        "Declare where HGP should sync this project, authenticate, and watch for changes.",
      supportTitle: "Repository source",
      supportDescription:
        "Repository-backed projects rely on the source adapter to detect providers, credentials, and polling posture.",
      supportCopy:
        "This source adapter lets the runtime poll a remote repository, assess access, and queue automation from new releases.",
      unsupportedMessage: null,
    };
  }

  return {
    kind: "local",
    stepLabel: "Workspace",
    stepDescription:
      "Declare the local workspace source that HGP should manage for this project.",
    supportTitle: "Local workspace source",
    supportDescription:
      "Local workspace projects point HGP at one host path that should be released without a managed repository checkout.",
    supportCopy:
      "Choose the Unity workspace path that HGP should inspect for versioning and build from this host directly.",
    unsupportedMessage: null,
  };
}

export function resolveBuildTargetWizardAdapter(
  engineKind: RepositoryEngineKind,
  projectKind: ProjectDraft["projectKind"],
): BuildTargetWizardAdapter {
  const engineLabel = formatRepositoryEngineKindLabel(engineKind);
  const projectLabel = formatProjectKindLabel(projectKind).toLocaleLowerCase();

  if (engineKind === "unity") {
    return {
      kind: "unity",
      stepLabel: "Build Targets",
      stepDescription: `Configure the ${engineLabel}-specific build targets HGP should execute for this ${projectLabel}.`,
      supportTitle: "Unity target adapter",
      supportDescription:
        "This step is currently being driven by the Unity build target adapter.",
      supportCopy:
        "Unity projects define the target platform, build method, and editor executable that HGP should launch for each build target.",
      reviewDescription:
        "Engine-specific target configuration that HGP will execute for this project.",
      unsupportedMessage: null,
    };
  }

  return {
    kind: "engine-unsupported",
    stepLabel: "Build Targets",
    stepDescription:
      "Configure the engine-specific build targets HGP should execute for this project.",
    supportTitle: `${engineLabel} target adapter`,
    supportDescription:
      "This step must switch to the adapter owned by the selected engine.",
    supportCopy: `${engineLabel} projects need a specialized build target adapter before project creation can collect engine-specific fields.`,
    reviewDescription: `Engine-specific target configuration for ${engineLabel} is not available in project creation yet.`,
    unsupportedMessage: `${engineLabel} build target setup does not have a create-project adapter yet.`,
  };
}

export function buildWizardSteps(
  sourceAdapter: ProjectSourceWizardAdapter,
  buildTargetAdapter: BuildTargetWizardAdapter,
): WizardStepDefinition[] {
  const definitions: Record<WizardStepKey, WizardStepDefinition> = {
    identity: {
      key: "identity",
      label: "Identity",
      description:
        "Name the project and choose the source and engine adapters HGP should use.",
    },
    access: {
      key: "access",
      label: sourceAdapter.stepLabel,
      description: sourceAdapter.stepDescription,
    },
    targets: {
      key: "targets",
      label: buildTargetAdapter.stepLabel,
      description: buildTargetAdapter.stepDescription,
    },
    publish: {
      key: "publish",
      label: "Publish Destinations",
      description:
        "Bind build outputs to publish destinations and validate destination-specific delivery rules before save.",
    },
    paths: {
      key: "paths",
      label: "Paths",
      description:
        "Choose optional artifact and workspace paths for this project.",
    },
    review: {
      key: "review",
      label: "Review",
      description:
        "Review the project definition produced by the selected source and engine adapters before registration.",
    },
  };

  return WIZARD_STEP_ORDER.map((stepKey) => definitions[stepKey]);
}

export function createInitialProjectDraft(): ProjectDraft {
  return {
    projectKind: "repository",
    engineKind: "unity",
    name: "",
    repositoryUrl: "",
    localPath: "",
    repositoryVisibility: "public",
    pollingIntervalSeconds: "300",
    artifactsRootOverride: "",
    workspaceRootOverride: "",
    unityExecutablePath: "",
    buildTargets: [],
    publishDestinations: [],
  };
}

export function cloneProjectDraft(draft: ProjectDraft): ProjectDraft {
  return {
    ...draft,
    buildTargets: draft.buildTargets.map((target) => ({
      ...target,
    })),
    publishDestinations: draft.publishDestinations.map((destination) => ({
      ...destination,
      bindings: destination.bindings.map((binding) => ({
        ...binding,
      })),
    })),
  };
}

export function resolveNextBuildTargetIndex(buildTargets: BuildTargetDraft[]) {
  return buildTargets.reduce((nextIndex, target) => {
    const match = /^target-(\d+)$/.exec(target.id.trim());

    if (!match) {
      return nextIndex;
    }

    return Math.max(nextIndex, Number(match[1]) + 1);
  }, 1);
}

export function normalizeUnityTargetPlatformValue(value: string) {
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

export function looksLikeAbsolutePath(value: string) {
  return (
    /^[a-zA-Z]:[\\/]/.test(value) ||
    value.startsWith("/") ||
    value.startsWith("\\\\")
  );
}

export function normalizePathForComparison(value: string) {
  const normalized = value.trim().replace(/\\/g, "/");

  if (normalized === "/" || /^[a-zA-Z]:\/$/.test(normalized)) {
    return normalized.toLocaleLowerCase();
  }

  return normalized.replace(/\/+$/, "").toLocaleLowerCase();
}

export function formatDiagnosticStatus(status: string) {
  switch (status) {
    case "ready":
      return "ready";
    case "missing_executable":
      return "missing";
    case "invalid_path":
      return "invalid";
    case "validation_failed":
      return "failed";
    default:
      return status.replace(/_/g, " ");
  }
}

export function formatGithubAuthProviderStatus(
  provider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
) {
  if (isLoadingAuthProviders) {
    return "loading";
  }

  if (!provider) {
    return "unavailable";
  }

  if (provider.status === "connected") {
    return "connected";
  }

  if (provider.status === "disconnected") {
    return "ready to connect";
  }

  return "unavailable";
}

export function resolveRepositoryAccessBadgeTone(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
): "strong" | "neutral" | "muted" {
  if (isAssessingRepositoryAccess) {
    return "muted";
  }

  if (repositoryAccessError) {
    return "neutral";
  }

  if (!assessment) {
    return "muted";
  }

  if (assessment.visibility === "public") {
    return "strong";
  }

  if (assessment.visibility === "private") {
    return assessment.supports_interactive_login ? "neutral" : "muted";
  }

  if (assessment.visibility === "invalid") {
    return "neutral";
  }

  return "muted";
}

export function formatRepositoryAccessStatus(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "pending";
  }

  if (isAssessingRepositoryAccess) {
    return "checking";
  }

  if (repositoryAccessError) {
    return "check failed";
  }

  if (!assessment) {
    return "pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "public";
    case "private":
      return assessment.supports_interactive_login
        ? "login required"
        : "unsupported";
    case "invalid":
      return "invalid";
    default:
      return "unknown";
  }
}

export function resolveRepositoryAccessCopy(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "Paste a repository URL, choose whether the repository is public or private, and HGP will detect which platform owns the host.";
  }

  if (isAssessingRepositoryAccess) {
    return "HGP is identifying which platform owns this repository URL and whether private login is supported for the selected visibility.";
  }

  if (repositoryAccessError) {
    return repositoryAccessError;
  }

  if (assessment) {
    return assessment.message;
  }

  return "Repository access has not been checked yet.";
}

export function formatRepositoryAccessSummary(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "Pending";
  }

  if (isAssessingRepositoryAccess) {
    return "Checking";
  }

  if (repositoryAccessError) {
    return "Check failed";
  }

  if (!assessment) {
    return "Pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "Public";
    case "private":
      return "Private";
    case "invalid":
      return "Invalid";
    default:
      return "Unknown";
  }
}

export function formatRepositoryAccessProviderLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (isAssessingRepositoryAccess) {
    return "Detecting";
  }

  if (repositoryAccessError || !assessment) {
    return "Pending";
  }

  return assessment.provider_label;
}

export function formatRepositoryVisibilityLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (isAssessingRepositoryAccess) {
    return "Checking";
  }

  if (repositoryAccessError) {
    return "Needs review";
  }

  if (!assessment) {
    return "Pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "Public";
    case "private":
      return "Private";
    case "invalid":
      return "Invalid";
    default:
      return "Unknown";
  }
}

export function formatRepositoryLoginStatus(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
) {
  if (!assessment) {
    return "Pending";
  }

  if (assessment.auth_requirement === "none") {
    return "Not required";
  }

  if (!assessment.supports_interactive_login) {
    return "Not available";
  }

  if (assessment.provider_id === "github") {
    return formatGithubAuthProviderStatus(
      githubAuthProvider,
      isLoadingAuthProviders,
    );
  }

  return "Required";
}

export function formatRepositoryBindingStatus(
  assessment: RepositoryAccessAssessment | null,
  repositoryCredentialId: number | null,
  pendingRepositoryAccessAction: boolean,
) {
  if (!assessment) {
    return "Pending";
  }

  if (pendingRepositoryAccessAction) {
    return "Connecting";
  }

  if (assessment.auth_requirement === "none") {
    return "Not required";
  }

  if (!supportsShellRepositoryLoginAction(assessment)) {
    return "Unavailable";
  }

  return repositoryCredentialId ? "Selected" : "Pending";
}

export function formatRepositoryBindingActionLabel(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  repositoryCredentialId: number | null,
) {
  if (!assessment) {
    return "Connect credential";
  }

  if (repositoryCredentialId) {
    return assessment.provider_id === "github"
      ? "Reconnect GitHub login"
      : "Change credential";
  }

  if (assessment.provider_id === "github") {
    return githubAuthProvider?.status === "connected"
      ? "Connect GitHub login"
      : "Log in and connect";
  }

  return "Select credential";
}

export function shouldShowRepositoryLoginAction(
  assessment: RepositoryAccessAssessment | null,
) {
  return supportsShellRepositoryLoginAction(assessment);
}

export function supportsShellRepositoryLoginAction(
  assessment: RepositoryAccessAssessment | null,
) {
  return Boolean(
    assessment?.auth_requirement === "required" &&
      assessment.supports_interactive_login &&
      assessment.provider_id === "github",
  );
}

export function formatRepositoryCredentialFieldHint(
  assessment: RepositoryAccessAssessment | null,
  isLoadingRepositoryCredentials: boolean,
) {
  if (isLoadingRepositoryCredentials) {
    return "Loading stored repository credentials...";
  }

  if (!assessment || assessment.auth_requirement !== "required") {
    return "Public repositories can keep this empty.";
  }

  return "Choose a stored GitHub credential or use the login action below.";
}

export function buildRepositoryAccessAssessmentFromDetection(
  detection: RepositoryProviderDetection,
  repositoryVisibility: ProjectDraft["repositoryVisibility"],
): RepositoryAccessAssessment {
  if (repositoryVisibility === "public") {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "public",
      auth_requirement: "none",
      auth_status: "not_required",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Public repository selected. HGP will poll and clone this remote without repository authentication.",
    };
  }

  if (
    detection.supports_interactive_login &&
    detection.provider_id === "github"
  ) {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "private",
      auth_requirement: "required",
      auth_status: "required_unbound",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Private GitHub repository selected. Log in and connect this project before saving.",
    };
  }

  if (detection.provider_id === "unknown") {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "private",
      auth_requirement: "required",
      auth_status: "unsupported",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Private repository selected, but HGP could not identify a supported login platform from this URL. Only public repositories are supported for this host right now.",
    };
  }

  return {
    provider_id: detection.provider_id,
    provider_label: detection.provider_label,
    instance_url: detection.instance_url,
    normalized_url: detection.normalized_url,
    visibility: "private",
    auth_requirement: "required",
    auth_status: "unsupported",
    supports_interactive_login: detection.supports_interactive_login,
    message: `Private ${detection.provider_label} repositories are not supported yet. Only public repositories are available for this platform right now.`,
  };
}

export function buildDetectedUnityEditorOptions(
  editors: DiscoveredUnityEditor[],
  isLoadingUnityAdapterSettings: boolean,
  unityAdapterSettingsError: string | null,
): SelectOption[] {
  if (isLoadingUnityAdapterSettings) {
    return [
      {
        label: "Scanning installed Unity editors...",
        value: "",
      },
    ];
  }

  if (unityAdapterSettingsError) {
    return [
      {
        label: "Unable to load installed Unity editors",
        value: "",
      },
    ];
  }

  if (editors.length === 0) {
    return [
      {
        label: "No installed Unity editors detected",
        value: "",
      },
    ];
  }

  return [
    {
      label: "Choose a detected Unity editor",
      title: "Choose a detected Unity editor",
      value: "",
    },
    ...editors.map((editor) => ({
      label: editor.version,
      title: editor.install_root_path,
      value: editor.executable_path,
    })),
  ];
}

export function buildDetectedUnityEditorHint(
  unityAdapterSettingsError: string | null,
  editorCount: number,
) {
  if (unityAdapterSettingsError) {
    return `${unityAdapterSettingsError} Use the manual path field below to continue.`;
  }

  if (editorCount === 0) {
    return "Choose a detected editor when available, or keep using the manual executable path field below.";
  }

  return "Select a detected Unity install to fill the executable path below, or keep using the manual picker.";
}

export function resolveDetectedUnityEditorValue(
  unityExecutablePath: string,
  editors: DiscoveredUnityEditor[],
) {
  const normalizedPath = unityExecutablePath.trim();

  return editors.some(
    (editor) => editor.executable_path.trim() === normalizedPath,
  )
    ? normalizedPath
    : "";
}

export function formatBuildTargetExecutableSummary(
  diagnostics: UnityExecutableValidation | null,
  isValidating: boolean,
) {
  if (isValidating) {
    return "checking";
  }

  if (!diagnostics) {
    return "pending";
  }

  return formatDiagnosticStatus(diagnostics.status);
}

export function buildBuildTargetQuickViewCopy(
  target: BuildTargetDraft,
  diagnostics: UnityExecutableValidation | null,
  unityExecutablePath: string,
) {
  if (diagnostics && diagnostics.status !== "ready") {
    return diagnostics.message;
  }

  if (!unityExecutablePath.trim()) {
    return "Unity executable path is still pending.";
  }

  return `${target.buildMethod.trim() || "Build method pending"} • ${unityExecutablePath.trim()}`;
}

export function formatProjectSourceReviewDescription(draft: ProjectDraft) {
  if (draft.projectKind === "repository") {
    return draft.repositoryUrl.trim() || "Repository source not set yet.";
  }

  return draft.localPath.trim() || "Local workspace source not set yet.";
}

type RepositoryAccessPanelProps = {
  repositoryUrl: string;
  repositoryAccessAssessment: RepositoryAccessAssessment | null;
  isAssessingRepositoryAccess: boolean;
  repositoryAccessError: string | null;
  repositoryAccessActionMessage?: string | null;
  validationError?: string;
  githubAuthProvider: AuthProviderStatus | null;
  isLoadingAuthProviders: boolean;
  authProviderError: string | null;
  isLoadingRepositoryCredentials: boolean;
  repositoryCredentialsError: string | null;
  repositoryCredentialId: number | null;
  repositoryCredentialOptions: SelectOption[];
  pendingRepositoryAccessAction: boolean;
  onRepositoryCredentialChange: (value: string) => void;
  onBindRepositoryAccess: () => void;
  onClearRepositoryAccessBinding?: () => void;
  onRetryRepositoryAccessCheck?: () => void;
  onRetryAuthProviders?: () => void;
  onRetryRepositoryCredentials?: () => void;
  onManageAuth?: () => void;
};

export function ProjectRepositoryAccessPanel({
  repositoryUrl,
  repositoryAccessAssessment,
  isAssessingRepositoryAccess,
  repositoryAccessError,
  repositoryAccessActionMessage,
  validationError,
  githubAuthProvider,
  isLoadingAuthProviders,
  authProviderError,
  isLoadingRepositoryCredentials,
  repositoryCredentialsError,
  repositoryCredentialId,
  repositoryCredentialOptions,
  pendingRepositoryAccessAction,
  onRepositoryCredentialChange,
  onBindRepositoryAccess,
  onClearRepositoryAccessBinding,
  onRetryRepositoryAccessCheck,
  onRetryAuthProviders,
  onRetryRepositoryCredentials,
  onManageAuth,
}: RepositoryAccessPanelProps) {
  return (
    <SurfacePanel
      actions={
        <Badge
          tone={resolveRepositoryAccessBadgeTone(
            repositoryAccessAssessment,
            isAssessingRepositoryAccess,
            repositoryAccessError,
          )}
        >
          {formatRepositoryAccessStatus(
            repositoryUrl,
            repositoryAccessAssessment,
            isAssessingRepositoryAccess,
            repositoryAccessError,
          )}
        </Badge>
      }
      className="wizard-support-panel"
      description={resolveRepositoryAccessCopy(
        repositoryUrl,
        repositoryAccessAssessment,
        isAssessingRepositoryAccess,
        repositoryAccessError,
      )}
      eyebrow="Repository"
      headerSeparated
      summary={
        repositoryUrl.trim() ||
        isAssessingRepositoryAccess ||
        repositoryAccessAssessment ||
        repositoryAccessError ? (
          <MetaRow className="wizard-callout__meta">
            <MetaItem label="Provider">
              {formatRepositoryAccessProviderLabel(
                repositoryAccessAssessment,
                isAssessingRepositoryAccess,
                repositoryAccessError,
              )}
            </MetaItem>
            <MetaItem label="Visibility">
              {formatRepositoryVisibilityLabel(
                repositoryAccessAssessment,
                isAssessingRepositoryAccess,
                repositoryAccessError,
              )}
            </MetaItem>
            <MetaItem label="Login">
              {formatRepositoryLoginStatus(
                repositoryAccessAssessment,
                githubAuthProvider,
                isLoadingAuthProviders,
              )}
            </MetaItem>
            <MetaItem label="Connection">
              {formatRepositoryBindingStatus(
                repositoryAccessAssessment,
                repositoryCredentialId,
                pendingRepositoryAccessAction,
              )}
            </MetaItem>
          </MetaRow>
        ) : undefined
      }
      title="Repository access"
      tone="inset"
    >
      {repositoryAccessActionMessage ? (
        <p className="feed-banner feed-banner--info">
          {repositoryAccessActionMessage}
        </p>
      ) : null}

      {validationError ? <p className="ui-field__error">{validationError}</p> : null}

      {repositoryAccessError || authProviderError || repositoryCredentialsError ? (
        <div className="wizard-callout__actions">
          {repositoryAccessError && onRetryRepositoryAccessCheck ? (
            <Button
              disabled={isAssessingRepositoryAccess}
              leadingIcon="refresh"
              onClick={onRetryRepositoryAccessCheck}
              size="sm"
              variant="secondary"
            >
              {isAssessingRepositoryAccess
                ? "Retrying access check..."
                : "Retry access check"}
            </Button>
          ) : null}

          {authProviderError && onRetryAuthProviders ? (
            <Button
              disabled={isLoadingAuthProviders}
              leadingIcon="refresh"
              onClick={onRetryAuthProviders}
              size="sm"
              variant="secondary"
            >
              {isLoadingAuthProviders
                ? "Retrying accounts..."
                : "Retry accounts"}
            </Button>
          ) : null}

          {repositoryCredentialsError && onRetryRepositoryCredentials ? (
            <Button
              disabled={isLoadingRepositoryCredentials}
              leadingIcon="refresh"
              onClick={onRetryRepositoryCredentials}
              size="sm"
              variant="secondary"
            >
              {isLoadingRepositoryCredentials
                ? "Retrying credentials..."
                : "Retry credentials"}
            </Button>
          ) : null}
        </div>
      ) : null}

      {shouldShowRepositoryLoginAction(repositoryAccessAssessment) ? (
        <>
          <SelectField
            disabled={
              isLoadingRepositoryCredentials || pendingRepositoryAccessAction
            }
            hint={formatRepositoryCredentialFieldHint(
              repositoryAccessAssessment,
              isLoadingRepositoryCredentials,
            )}
            label="Repository credential"
            onChange={(event) =>
              onRepositoryCredentialChange(event.currentTarget.value)
            }
            options={repositoryCredentialOptions}
            value={repositoryCredentialId?.toString() ?? ""}
          />

          <div className="wizard-callout__actions">
            {supportsShellRepositoryLoginAction(repositoryAccessAssessment) ? (
              <Button
                disabled={pendingRepositoryAccessAction}
                leadingIcon="key"
                onClick={onBindRepositoryAccess}
                size="sm"
                variant={
                  repositoryCredentialId !== null ? "secondary" : "primary"
                }
              >
                {pendingRepositoryAccessAction
                  ? "Connecting login..."
                  : formatRepositoryBindingActionLabel(
                      repositoryAccessAssessment,
                      githubAuthProvider,
                      repositoryCredentialId,
                    )}
              </Button>
            ) : null}

            {repositoryCredentialId !== null && onClearRepositoryAccessBinding ? (
              <Button
                onClick={onClearRepositoryAccessBinding}
                size="sm"
                variant="ghost"
              >
                Disconnect
              </Button>
            ) : null}

            {onManageAuth &&
            supportsShellRepositoryLoginAction(repositoryAccessAssessment) ? (
              <Button onClick={onManageAuth} size="sm" variant="ghost">
                Open accounts
              </Button>
            ) : null}
          </div>
        </>
      ) : null}
    </SurfacePanel>
  );
}

type ProjectIdentityStepProps = {
  draft: Pick<ProjectDraft, "name" | "projectKind" | "engineKind">;
  errors?: {
    name?: string;
    projectKind?: string;
    engineKind?: string;
  };
  onNameChange: (value: string) => void;
  onProjectKindChange: (value: ProjectDraft["projectKind"]) => void;
  onEngineKindChange: (value: RepositoryEngineKind) => void;
  onFieldBlur?: (fieldName: "name" | "projectKind" | "engineKind") => void;
};

export function ProjectIdentityStep({
  draft,
  errors,
  onNameChange,
  onProjectKindChange,
  onEngineKindChange,
  onFieldBlur,
}: ProjectIdentityStepProps) {
  return (
    <div className="wizard-form-grid">
      <TextField
        error={errors?.name}
        label="Project name"
        onBlur={() => onFieldBlur?.("name")}
        onChange={(event) => onNameChange(event.currentTarget.value)}
        placeholder="Red Horizon"
        value={draft.name}
      />

      <SelectField
        error={errors?.projectKind}
        label="Project kind"
        onBlur={() => onFieldBlur?.("projectKind")}
        onChange={(event) =>
          onProjectKindChange(
            event.currentTarget.value as ProjectDraft["projectKind"],
          )
        }
        options={PROJECT_KIND_OPTIONS}
        value={draft.projectKind}
      />

      <RepositoryEngineField
        error={errors?.engineKind}
        onBlur={() => onFieldBlur?.("engineKind")}
        onChange={(event) =>
          onEngineKindChange(
            event.currentTarget.value as RepositoryEngineKind,
          )
        }
        value={draft.engineKind}
      />
    </div>
  );
}

type ProjectRepositoryAccessStepProps = {
  repositoryUrl: string;
  repositoryVisibility: ProjectDraft["repositoryVisibility"];
  pollingIntervalSeconds: string;
  repositoryUrlError?: string;
  pollingIntervalSecondsError?: string;
  onRepositoryUrlChange: (value: string) => void;
  onRepositoryVisibilityChange: (
    value: ProjectDraft["repositoryVisibility"],
  ) => void;
  onPollingIntervalSecondsChange: (value: string) => void;
  onRepositoryUrlBlur?: () => void;
  onRepositoryVisibilityBlur?: () => void;
  onPollingIntervalSecondsBlur?: () => void;
  repositoryAccessPanel: ReactNode;
};

export function ProjectRepositoryAccessStep({
  repositoryUrl,
  repositoryVisibility,
  pollingIntervalSeconds,
  repositoryUrlError,
  pollingIntervalSecondsError,
  onRepositoryUrlChange,
  onRepositoryVisibilityChange,
  onPollingIntervalSecondsChange,
  onRepositoryUrlBlur,
  onRepositoryVisibilityBlur,
  onPollingIntervalSecondsBlur,
  repositoryAccessPanel,
}: ProjectRepositoryAccessStepProps) {
  return (
    <>
      <div className="wizard-form-grid">
        <TextField
          error={repositoryUrlError}
          hint="Use the HTTPS remote that HGP will poll and clone."
          label="Repository URL"
          leadingIcon="server"
          onBlur={onRepositoryUrlBlur}
          onChange={(event) => onRepositoryUrlChange(event.currentTarget.value)}
          placeholder="https://github.com/org/project.git"
          value={repositoryUrl}
        />

        <SelectField
          hint="Tell HGP whether this remote should be treated as public or private."
          label="Repository visibility"
          onBlur={onRepositoryVisibilityBlur}
          onChange={(event) =>
            onRepositoryVisibilityChange(
              event.currentTarget.value as ProjectDraft["repositoryVisibility"],
            )
          }
          options={REPOSITORY_VISIBILITY_OPTIONS}
          value={repositoryVisibility}
        />

        <TextField
          error={pollingIntervalSecondsError}
          hint="Polling stays operator-visible. The runtime requires at least 5 seconds."
          label="Polling interval (seconds)"
          min={5}
          onBlur={onPollingIntervalSecondsBlur}
          onChange={(event) =>
            onPollingIntervalSecondsChange(event.currentTarget.value)
          }
          step={5}
          type="number"
          value={pollingIntervalSeconds}
        />
      </div>

      {repositoryAccessPanel}
    </>
  );
}

type ProjectLocalAccessStepProps = {
  localPath: string;
  localPathError?: string;
  disabled?: boolean;
  onClearLocalPath: () => void;
  onPathPickError: (error: unknown) => void;
  onPathPicked: (selectedPath: string) => void;
};

export function ProjectLocalAccessStep({
  localPath,
  localPathError,
  disabled = false,
  onClearLocalPath,
  onPathPickError,
  onPathPicked,
}: ProjectLocalAccessStepProps) {
  return (
    <div className="wizard-form-grid">
      <PathPickerField
        buttonLabel="Choose workspace"
        clearable
        disabled={disabled}
        dialogTitle="Select local workspace directory"
        error={localPathError}
        hint="Choose the host-local Unity workspace that HGP should build directly."
        label="Local workspace path"
        onClear={onClearLocalPath}
        onError={onPathPickError}
        onPathPicked={onPathPicked}
        pickerKind="directory"
        placeholder="C:/projects/red-horizon"
        value={localPath}
      />
    </div>
  );
}

type ProjectTargetsStepProps = {
  buildTargetAdapter: BuildTargetWizardAdapter;
  buildTargets: BuildTargetDraft[];
  rootError?: string;
  targetErrors?: Record<string, TargetFieldErrors>;
  removalCallout?: ReactNode;
  detectedEditorOptions: SelectOption[];
  detectedEditorValue: string;
  detectedEditorHint: string;
  detectedEditorDisabled: boolean;
  onDetectedEditorChange: (selectedPath: string) => void;
  unityExecutablePath: string;
  unityExecutableError?: string;
  onUnityExecutablePicked: (selectedPath: string) => void;
  onUnityExecutablePickError: (error: unknown) => void;
  unityExecutableDiagnostics: UnityExecutableValidation | null;
  isValidatingUnityExecutable: boolean;
  isBusy: boolean;
  onEditTarget: (targetId: string) => void;
  onRemoveTarget: (targetId: string) => void;
  onAddTarget: () => void;
};

export function ProjectTargetsStep({
  buildTargetAdapter,
  buildTargets,
  rootError,
  targetErrors = {},
  removalCallout,
  detectedEditorOptions,
  detectedEditorValue,
  detectedEditorHint,
  detectedEditorDisabled,
  onDetectedEditorChange,
  unityExecutablePath,
  unityExecutableError,
  onUnityExecutablePicked,
  onUnityExecutablePickError,
  unityExecutableDiagnostics,
  isValidatingUnityExecutable,
  isBusy,
  onEditTarget,
  onRemoveTarget,
  onAddTarget,
}: ProjectTargetsStepProps) {
  if (buildTargetAdapter.kind !== "unity") {
    return (
      <div className="wizard-form-grid">
        {rootError ? (
          <p className="feed-banner feed-banner--error">{rootError}</p>
        ) : null}

        <div className="wizard-callout wizard-callout--compact">
          <p className="wizard-callout__copy">
            {buildTargetAdapter.unsupportedMessage ?? buildTargetAdapter.supportCopy}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="wizard-targets-shell">
      {rootError ? <p className="feed-banner feed-banner--error">{rootError}</p> : null}
      {removalCallout}

      <SelectField
        disabled={detectedEditorDisabled}
        hint={detectedEditorHint}
        label="Installed Unity editors"
        onChange={(event) => {
          const selectedPath = event.currentTarget.value.trim();
          if (!selectedPath) {
            return;
          }

          onDetectedEditorChange(selectedPath);
        }}
        options={detectedEditorOptions}
        value={detectedEditorValue}
      />

      <PathPickerField
        buttonLabel="Choose Unity executable"
        dialogTitle="Select Unity Editor executable"
        error={unityExecutableError}
        filters={[
          {
            extensions: ["exe", "app"],
            name: "Unity Editor",
          },
        ]}
        hint="Select the host-local Unity Editor executable that should run every build target in this project."
        label="Unity executable"
        onError={onUnityExecutablePickError}
        onPathPicked={onUnityExecutablePicked}
        pickerKind="file"
        placeholder="C:/Program Files/Unity/Hub/Editor/.../Unity.exe"
        value={unityExecutablePath}
      />

      {unityExecutableDiagnostics ? (
        <p
          className={joinClassNames(
            "wizard-target-card__diagnostic",
            unityExecutableDiagnostics.status !== "ready" &&
              "wizard-target-card__diagnostic--error",
          )}
        >
          {unityExecutableDiagnostics.message}
        </p>
      ) : null}

      {isValidatingUnityExecutable ? (
        <p className="wizard-target-card__diagnostic">
          Validating Unity executable path...
        </p>
      ) : null}

      {buildTargets.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">No build targets configured.</p>
        </div>
      ) : null}

      {buildTargets.map((target, index) => {
        const fieldErrors = targetErrors[target.id] ?? {};
        const errorPreview =
          fieldErrors.name ||
          fieldErrors.targetPlatform ||
          fieldErrors.buildMethod ||
          null;

        return (
          <SurfacePanel
            actions={
              <div className="publish-destination-quick-view__actions">
                <Button
                  disabled={isBusy}
                  onClick={() => onEditTarget(target.id)}
                  size="sm"
                  variant="ghost"
                >
                  Edit
                </Button>
                <Button
                  disabled={isBusy}
                  leadingIcon="trash"
                  onClick={() => onRemoveTarget(target.id)}
                  size="sm"
                  variant="ghost"
                >
                  Remove
                </Button>
              </div>
            }
            className="publish-destination-quick-view"
            key={target.id}
            summary={
              <MetaRow className="wizard-target-card__summary">
                <MetaItem label="Platform">
                  {target.targetPlatform.trim() || "pending"}
                </MetaItem>
                <MetaItem label="Build method">
                  {target.buildMethod.trim() || "pending"}
                </MetaItem>
                <MetaItem label="Unity executable">
                  {formatBuildTargetExecutableSummary(
                    unityExecutableDiagnostics,
                    isValidatingUnityExecutable,
                  )}
                </MetaItem>
              </MetaRow>
            }
            title={target.name.trim() || `Build target ${index + 1}`}
            tone="inset"
          >
            {errorPreview ? (
              <p className="ui-field__error">{errorPreview}</p>
            ) : (
              <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                {buildBuildTargetQuickViewCopy(
                  target,
                  unityExecutableDiagnostics,
                  unityExecutablePath,
                )}
              </p>
            )}
          </SurfacePanel>
        );
      })}

      <div className="wizard-targets-shell__footer">
        <Button
          leadingIcon="plus"
          onClick={onAddTarget}
          size="sm"
          variant="secondary"
        >
          Add target
        </Button>
      </div>
    </div>
  );
}

type ProjectPublishStepProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  destinations: PublishDestinationDraft[];
  disabled: boolean;
  errors?: PublishDestinationValidationErrors;
  showItchUserversionTemplate: boolean;
  onChange: (nextPublishDestinations: PublishDestinationDraft[]) => void;
  onSaveCredential?: (
    destinationId: string,
    input: SaveSecretCredentialInput,
  ) => Promise<number> | number;
};

export function ProjectPublishStep({
  buildTargets,
  credentials,
  destinations,
  disabled,
  errors,
  showItchUserversionTemplate,
  onChange,
  onSaveCredential,
}: ProjectPublishStepProps) {
  return (
    <PublishDestinationsEditor
      buildTargets={buildTargets}
      credentials={credentials}
      destinations={destinations}
      disabled={disabled}
      editingMode="overlay"
      errors={errors}
      onChange={onChange}
      showItchUserversionTemplate={showItchUserversionTemplate}
      onSaveCredential={onSaveCredential}
    />
  );
}

type ProjectPathsStepProps = {
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  artifactsRootOverrideError?: string;
  workspaceRootOverrideError?: string;
  disabled?: boolean;
  onArtifactsRootClear: () => void;
  onWorkspaceRootClear: () => void;
  onPathPickError: (error: unknown) => void;
  onArtifactsRootPicked: (selectedPath: string) => void;
  onWorkspaceRootPicked: (selectedPath: string) => void;
};

export function ProjectPathsStep({
  artifactsRootOverride,
  workspaceRootOverride,
  artifactsRootOverrideError,
  workspaceRootOverrideError,
  disabled = false,
  onArtifactsRootClear,
  onWorkspaceRootClear,
  onPathPickError,
  onArtifactsRootPicked,
  onWorkspaceRootPicked,
}: ProjectPathsStepProps) {
  return (
    <div className="wizard-form-grid">
      <PathPickerField
        buttonLabel="Choose artifacts root"
        clearable
        disabled={disabled}
        dialogTitle="Select artifacts root directory"
        error={artifactsRootOverrideError}
        hint="Optional. Override the artifact root for this repository only."
        label="Artifacts root override"
        onClear={onArtifactsRootClear}
        onError={onPathPickError}
        onPathPicked={onArtifactsRootPicked}
        pickerKind="directory"
        placeholder="C:/builds/red-horizon"
        value={artifactsRootOverride}
      />

      <PathPickerField
        buttonLabel="Choose workspace root"
        clearable
        disabled={disabled}
        dialogTitle="Select managed workspace root directory"
        error={workspaceRootOverrideError}
        hint="Optional. Override the managed checkout root for this repository only."
        label="Workspace root override"
        onClear={onWorkspaceRootClear}
        onError={onPathPickError}
        onPathPicked={onWorkspaceRootPicked}
        pickerKind="directory"
        placeholder="C:/workspaces/red-horizon"
        value={workspaceRootOverride}
      />
    </div>
  );
}

type ProjectReviewStepProps = {
  draft: ProjectDraft;
  buildTargetAdapter: BuildTargetWizardAdapter;
  projectSourceStepAdapter: ProjectSourceWizardAdapter;
  repositoryAccessSummary: string;
  unityExecutableDiagnostics: UnityExecutableValidation | null;
  isValidatingUnityExecutable: boolean;
};

export function ProjectReviewStep({
  draft,
  buildTargetAdapter,
  projectSourceStepAdapter,
  repositoryAccessSummary,
  unityExecutableDiagnostics,
  isValidatingUnityExecutable,
}: ProjectReviewStepProps) {
  const publishDestinationReviewSummary = buildPublishDestinationReviewSummary(
    draft.publishDestinations,
    draft.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId ?? null,
      name: target.name.trim() || "Unnamed target",
    })),
  );
  const unboundPublishTargetNames = listUnboundBuildTargetNames(
    draft.publishDestinations,
    draft.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId ?? null,
      name: target.name.trim() || "Unnamed target",
    })),
  );

  return (
    <div className="wizard-review-shell">
      <SurfacePanel
        className="wizard-review-panel"
        description={formatProjectSourceReviewDescription(draft)}
        eyebrow="Project"
        headerSeparated
        summary={
          <MetaRow>
            <MetaItem label="Engine">
              {formatRepositoryEngineKindLabel(draft.engineKind)}
            </MetaItem>
            {draft.projectKind === "repository" ? (
              <MetaItem label="Poll">
                {`${draft.pollingIntervalSeconds.trim() || "0"}s`}
              </MetaItem>
            ) : (
              <MetaItem label="Source">No remote polling</MetaItem>
            )}
            <MetaItem
              label={draft.projectKind === "repository" ? "Access" : "Source"}
            >
              {draft.projectKind === "repository"
                ? repositoryAccessSummary
                : formatProjectSourceAdapterStatus(projectSourceStepAdapter)}
            </MetaItem>
          </MetaRow>
        }
        title={draft.name.trim() || "Unnamed project"}
        tone="inset"
      >
        <p className="wizard-summary-panel__copy">
          {formatProjectKindLabel(draft.projectKind)} with
          {` ${formatWizardTargetCount(draft.buildTargets.length)} configured for registration.`}
        </p>
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description={buildTargetAdapter.reviewDescription}
        eyebrow="Build Targets"
        headerSeparated
        title="Target Review"
        tone="inset"
      >
        {buildTargetAdapter.kind === "unity" ? (
          <div className="wizard-summary-list">
            {draft.buildTargets.map((target) => (
              <div className="wizard-summary-list__item" key={target.id}>
                <div className="wizard-summary-list__title-row">
                  <strong>{target.name.trim() || "Unnamed target"}</strong>
                  <Badge tone="neutral">
                    {target.targetPlatform || "Unity target pending"}
                  </Badge>
                </div>
                <p className="wizard-summary-list__copy">
                  {target.buildMethod.trim() || "Unity build method pending"}
                </p>
              </div>
            ))}
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>Shared Unity executable</strong>
                <Badge tone="muted">
                  {formatBuildTargetExecutableSummary(
                    unityExecutableDiagnostics,
                    isValidatingUnityExecutable,
                  )}
                </Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                {draft.unityExecutablePath.trim() || "Unity executable pending"}
              </p>
            </div>
          </div>
        ) : (
          <div className="wizard-summary-list">
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>{buildTargetAdapter.supportTitle}</strong>
                <Badge tone="muted">unavailable</Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                {buildTargetAdapter.unsupportedMessage ?? buildTargetAdapter.supportCopy}
              </p>
            </div>
          </div>
        )}
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description="Destination-specific publish bindings and credential readiness."
        eyebrow="Publish Destinations"
        headerSeparated
        title="Destination Review"
        tone="inset"
      >
        <div className="wizard-summary-list">
          {publishDestinationReviewSummary.length === 0 ? (
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>No publish destinations configured</strong>
                <Badge tone="muted">valid</Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                Every build target will keep its artifact under the runtime-managed output root.
              </p>
            </div>
          ) : (
            publishDestinationReviewSummary.map((destination) => (
              <div className="wizard-summary-list__item" key={destination.id}>
                <div className="wizard-summary-list__title-row">
                  <strong>{destination.name}</strong>
                  <Badge tone={destination.enabled ? "strong" : "muted"}>
                    {destination.kindLabel}
                  </Badge>
                </div>
                <p className="wizard-summary-list__copy">
                  {destination.bindingTargetNames.length > 0
                    ? destination.bindingTargetNames.join(", ")
                    : "No build targets bound yet."}
                </p>
                <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                  {destination.missingCredential
                    ? "Credential still missing."
                    : "Uploads are managed automatically by HGP for the selected channels."}
                </p>
              </div>
            ))
          )}

          <div className="wizard-summary-list__item">
            <div className="wizard-summary-list__title-row">
              <strong>Unbound build targets</strong>
              <Badge tone="muted">
                {unboundPublishTargetNames.length === 0 ? "none" : "kept local"}
              </Badge>
            </div>
            <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
              {unboundPublishTargetNames.length > 0
                ? unboundPublishTargetNames.join(", ")
                : "Every configured build target is bound to at least one publish destination."}
            </p>
          </div>
        </div>
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description="Project-specific overrides for artifacts and managed workspaces."
        eyebrow="Paths"
        headerSeparated
        title="Path Review"
        tone="inset"
      >
        <div className="wizard-summary-list">
          <div className="wizard-summary-list__item">
            <div className="wizard-summary-list__title-row">
              <strong>Artifacts root</strong>
              <Badge tone="muted">
                {draft.artifactsRootOverride.trim() ? "override" : "default"}
              </Badge>
            </div>
            <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
              {draft.artifactsRootOverride.trim() ||
                "Use the runtime default artifact root."}
            </p>
          </div>

          <div className="wizard-summary-list__item">
            <div className="wizard-summary-list__title-row">
              <strong>Workspace root</strong>
              <Badge tone="muted">
                {draft.workspaceRootOverride.trim() ? "override" : "default"}
              </Badge>
            </div>
            <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
              {draft.workspaceRootOverride.trim() ||
                "Use the runtime default managed checkout root."}
            </p>
          </div>
        </div>
      </SurfacePanel>
    </div>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}