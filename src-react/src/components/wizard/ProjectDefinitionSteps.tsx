import { type ReactNode } from "react";

import { Button } from "../Button";
import { SelectField, TextField, type SelectOption } from "../Field";
import { PathPickerField } from "../PathPickerField";
import {
  PublishDestinationsEditor,
  buildPublishDestinationReviewSummary,
  listUnboundBuildTargetNames,
  type PublishDestinationEditingMode,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
  type PublishDestinationValidationErrors,
} from "../PublishDestinationsEditor";
import { RepositoryEngineField } from "../RepositoryEngineField";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "../Surface";
import { type AuthProviderStatus } from "../../services/auth";
import {
  type BuildProcessPriority,
  type DiscoveredUnityEditor,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryProviderDetection,
  type SaveSecretCredentialInput,
  type SecretCredentialSetting,
  type UnityExecutableValidation,
} from "../../services/projects";
import {
  useLocalization,
  type LocalizationVariables,
  type Translate,
} from "../../LocalizationProvider";

export type BuildTargetDraft = {
  id: string;
  name: string;
  targetPlatform: string;
  buildMethod: string;
  buildTargetId?: number | null;
  processPriority?: BuildProcessPriority;
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
  workspaceRootOverride: string;
  processPriority: BuildProcessPriority;
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

function translateMessage(
  t: Translate | undefined,
  key: string,
  fallbackText: string,
  variables?: LocalizationVariables,
) {
  if (t) {
    return t(key, fallbackText, variables);
  }

  if (!variables) {
    return fallbackText;
  }

  return fallbackText.replace(/\{\{\s*([\w.]+)\s*\}\}/g, (match, name) => {
    const value = variables[name];
    return value === undefined ? match : String(value);
  });
}

function buildProjectKindOptions(t: Translate): ReadonlyArray<SelectOption> {
  return [
    {
      label: translateMessage(
        t,
        "project_shared.kind.repository",
        "Repository project",
      ),
      value: "repository",
    },
    {
      label: translateMessage(
        t,
        "project_shared.kind.local_workspace",
        "Local workspace project",
      ),
      value: "local",
    },
  ];
}

function buildRepositoryVisibilityOptions(
  t: Translate,
): ReadonlyArray<SelectOption> {
  return [
    {
      label: translateMessage(t, "project_shared.visibility.public", "Public"),
      value: "public",
    },
    {
      label: translateMessage(
        t,
        "project_shared.visibility.private",
        "Private",
      ),
      value: "private",
    },
  ];
}

export function formatProjectKindLabel(
  projectKind: ProjectDraft["projectKind"],
  t?: Translate,
) {
  return projectKind === "repository"
    ? translateMessage(
        t,
        "project_shared.kind.repository",
        "Repository project",
      )
    : translateMessage(
        t,
        "project_shared.kind.local_workspace",
        "Local workspace project",
      );
}

export function formatRepositoryEngineKindLabel(
  engineKind: RepositoryEngineKind,
  t?: Translate,
) {
  switch (engineKind) {
    case "unity":
      return translateMessage(t, "project_shared.engine.option.unity", "Unity");
    case "unreal":
      return translateMessage(
        t,
        "project_shared.engine.option.unreal",
        "Unreal",
      );
    case "godot":
      return translateMessage(t, "project_shared.engine.option.godot", "Godot");
    case "gamemaker":
      return translateMessage(
        t,
        "project_shared.engine.option.gamemaker",
        "GameMaker",
      );
    case "defold":
      return translateMessage(
        t,
        "project_shared.engine.option.defold",
        "Defold",
      );
    case "cocos-creator":
      return translateMessage(
        t,
        "project_shared.engine.option.cocos_creator",
        "Cocos Creator",
      );
  }

  const fallbackLabel = String(engineKind);

  return fallbackLabel
    .split("-")
    .map((segment: string) =>
      segment.length > 0
        ? `${segment[0].toUpperCase()}${segment.slice(1)}`
        : segment,
    )
    .join(" ");
}

export function formatWizardTargetCount(targetCount: number, t?: Translate) {
  return targetCount === 1
    ? translateMessage(t, "project_shared.count.target.one", "1 target")
    : translateMessage(
        t,
        "project_shared.count.target.other",
        "{{count}} targets",
        { count: targetCount },
      );
}

export function formatProjectSourceAdapterStatus(
  adapter: ProjectSourceWizardAdapter,
  t?: Translate,
) {
  return adapter.kind === "repository" || adapter.kind === "local"
    ? translateMessage(
        t,
        "project_shared.source_adapter.available",
        "Available",
      )
    : translateMessage(
        t,
        "project_shared.source_adapter.unavailable",
        "Unavailable",
      );
}

export function resolveProjectSourceWizardAdapter(
  projectKind: ProjectDraft["projectKind"],
  t?: Translate,
): ProjectSourceWizardAdapter {
  if (projectKind === "repository") {
    return {
      kind: "repository",
      stepLabel: translateMessage(
        t,
        "project_shared.source.repository.step_label",
        "Repository",
      ),
      stepDescription: translateMessage(
        t,
        "project_shared.source.repository.step_description",
        "Declare where HGP should sync this project, authenticate, and watch for changes.",
      ),
      supportTitle: translateMessage(
        t,
        "project_shared.source.repository.support_title",
        "Repository source",
      ),
      supportDescription: translateMessage(
        t,
        "project_shared.source.repository.support_description",
        "Repository-backed projects rely on the source adapter to detect providers, credentials, and polling posture.",
      ),
      supportCopy: translateMessage(
        t,
        "project_shared.source.repository.support_copy",
        "This source adapter lets the runtime poll a remote repository, assess access, and queue automation from new releases.",
      ),
      unsupportedMessage: null,
    };
  }

  return {
    kind: "local",
    stepLabel: translateMessage(
      t,
      "project_shared.source.local.step_label",
      "Workspace",
    ),
    stepDescription: translateMessage(
      t,
      "project_shared.source.local.step_description",
      "Declare the local workspace source that HGP should manage for this project.",
    ),
    supportTitle: translateMessage(
      t,
      "project_shared.source.local.support_title",
      "Local workspace source",
    ),
    supportDescription: translateMessage(
      t,
      "project_shared.source.local.support_description",
      "Local workspace projects point HGP at one host path that should be released without a managed repository checkout.",
    ),
    supportCopy: translateMessage(
      t,
      "project_shared.source.local.support_copy",
      "Choose the Unity workspace path that HGP should inspect for versioning and build from this host directly.",
    ),
    unsupportedMessage: null,
  };
}

export function resolveBuildTargetWizardAdapter(
  engineKind: RepositoryEngineKind,
  projectKind: ProjectDraft["projectKind"],
  t?: Translate,
): BuildTargetWizardAdapter {
  const engineLabel = formatRepositoryEngineKindLabel(engineKind, t);
  const projectLabel = formatProjectKindLabel(
    projectKind,
    t,
  ).toLocaleLowerCase();

  if (engineKind === "unity") {
    return {
      kind: "unity",
      stepLabel: translateMessage(
        t,
        "project_shared.build_target.unity.step_label",
        "Build Targets",
      ),
      stepDescription: translateMessage(
        t,
        "project_shared.build_target.unity.step_description",
        "Configure the {{engineLabel}}-specific build targets HGP should execute for this {{projectLabel}}.",
        {
          engineLabel,
          projectLabel,
        },
      ),
      supportTitle: translateMessage(
        t,
        "project_shared.build_target.unity.support_title",
        "Unity target adapter",
      ),
      supportDescription: translateMessage(
        t,
        "project_shared.build_target.unity.support_description",
        "This step is currently being driven by the Unity build target adapter.",
      ),
      supportCopy: translateMessage(
        t,
        "project_shared.build_target.unity.support_copy",
        "Unity projects define the target platform, build method, and editor executable that HGP should launch for each build target.",
      ),
      reviewDescription: translateMessage(
        t,
        "project_shared.build_target.unity.review_description",
        "Engine-specific target configuration that HGP will execute for this project.",
      ),
      unsupportedMessage: null,
    };
  }

  return {
    kind: "engine-unsupported",
    stepLabel: translateMessage(
      t,
      "project_shared.build_target.unsupported.step_label",
      "Build Targets",
    ),
    stepDescription: translateMessage(
      t,
      "project_shared.build_target.unsupported.step_description",
      "Configure the engine-specific build targets HGP should execute for this project.",
    ),
    supportTitle: translateMessage(
      t,
      "project_shared.build_target.unsupported.support_title",
      "{{engineLabel}} target adapter",
      { engineLabel },
    ),
    supportDescription: translateMessage(
      t,
      "project_shared.build_target.unsupported.support_description",
      "This step must switch to the adapter owned by the selected engine.",
    ),
    supportCopy: translateMessage(
      t,
      "project_shared.build_target.unsupported.support_copy",
      "{{engineLabel}} projects need a specialized build target adapter before project creation can collect engine-specific fields.",
      { engineLabel },
    ),
    reviewDescription: translateMessage(
      t,
      "project_shared.build_target.unsupported.review_description",
      "Engine-specific target configuration for {{engineLabel}} is not available in project creation yet.",
      { engineLabel },
    ),
    unsupportedMessage: translateMessage(
      t,
      "project_shared.build_target.unsupported.message",
      "{{engineLabel}} build target setup does not have a create-project adapter yet.",
      { engineLabel },
    ),
  };
}

export function buildWizardSteps(
  sourceAdapter: ProjectSourceWizardAdapter,
  buildTargetAdapter: BuildTargetWizardAdapter,
  t?: Translate,
): WizardStepDefinition[] {
  const definitions: Record<WizardStepKey, WizardStepDefinition> = {
    identity: {
      key: "identity",
      label: translateMessage(
        t,
        "project_shared.step.identity.label",
        "Identity",
      ),
      description: translateMessage(
        t,
        "project_shared.step.identity.description",
        "Name the project and choose the source and engine adapters HGP should use.",
      ),
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
      label: translateMessage(
        t,
        "project_shared.step.publish.label",
        "Publish Destinations",
      ),
      description: translateMessage(
        t,
        "project_shared.step.publish.description",
        "Bind build outputs to publish destinations and validate destination-specific delivery rules before save.",
      ),
    },
    paths: {
      key: "paths",
      label: translateMessage(t, "project_shared.step.paths.label", "Paths"),
      description: translateMessage(
        t,
        "project_shared.step.paths.description",
        "Review the managed workspace path HGP should use for this project.",
      ),
    },
    review: {
      key: "review",
      label: translateMessage(t, "project_shared.step.review.label", "Review"),
      description: translateMessage(
        t,
        "project_shared.step.review.description",
        "Review the project definition produced by the selected source and engine adapters before registration.",
      ),
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
    workspaceRootOverride: "",
    processPriority: "low",
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

export function formatDiagnosticStatus(status: string, t?: Translate) {
  switch (status) {
    case "ready":
      return translateMessage(t, "project_shared.diagnostic.ready", "ready");
    case "missing_executable":
      return translateMessage(
        t,
        "project_shared.diagnostic.missing",
        "missing",
      );
    case "invalid_path":
      return translateMessage(
        t,
        "project_shared.diagnostic.invalid",
        "invalid",
      );
    case "validation_failed":
      return translateMessage(t, "project_shared.diagnostic.failed", "failed");
    default:
      return status.replace(/_/g, " ");
  }
}

export function formatGithubAuthProviderStatus(
  provider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
  t?: Translate,
) {
  if (isLoadingAuthProviders) {
    return translateMessage(
      t,
      "project_shared.github_auth_status.loading",
      "loading",
    );
  }

  if (!provider) {
    return translateMessage(
      t,
      "project_shared.github_auth_status.unavailable",
      "unavailable",
    );
  }

  if (provider.status === "connected") {
    return translateMessage(
      t,
      "project_shared.github_auth_status.connected",
      "connected",
    );
  }

  if (provider.status === "disconnected") {
    return translateMessage(
      t,
      "project_shared.github_auth_status.disconnected",
      "ready to connect",
    );
  }

  return translateMessage(
    t,
    "project_shared.github_auth_status.unavailable",
    "unavailable",
  );
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
  t?: Translate,
) {
  if (!repositoryUrl.trim()) {
    return translateMessage(
      t,
      "project_shared.repository_access.status.pending",
      "pending",
    );
  }

  if (isAssessingRepositoryAccess) {
    return translateMessage(
      t,
      "project_shared.repository_access.status.checking",
      "checking",
    );
  }

  if (repositoryAccessError) {
    return translateMessage(
      t,
      "project_shared.repository_access.status.check_failed",
      "check failed",
    );
  }

  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.status.pending",
      "pending",
    );
  }

  switch (assessment.visibility) {
    case "public":
      return translateMessage(
        t,
        "project_shared.repository_access.status.public",
        "public",
      );
    case "private":
      return assessment.supports_interactive_login
        ? translateMessage(
            t,
            "project_shared.repository_access.status.login_required",
            "login required",
          )
        : translateMessage(
            t,
            "project_shared.repository_access.status.unsupported",
            "unsupported",
          );
    case "invalid":
      return translateMessage(
        t,
        "project_shared.repository_access.status.invalid",
        "invalid",
      );
    default:
      return translateMessage(
        t,
        "project_shared.repository_access.status.unknown",
        "unknown",
      );
  }
}

export function resolveRepositoryAccessCopy(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
  t?: Translate,
) {
  if (!repositoryUrl.trim()) {
    return translateMessage(
      t,
      "project_shared.repository_access.copy.empty",
      "Paste a repository URL, choose whether the repository is public or private, and HGP will detect which platform owns the host.",
    );
  }

  if (isAssessingRepositoryAccess) {
    return translateMessage(
      t,
      "project_shared.repository_access.copy.checking",
      "HGP is identifying which platform owns this repository URL and whether private login is supported for the selected visibility.",
    );
  }

  if (repositoryAccessError) {
    return repositoryAccessError;
  }

  if (assessment) {
    return assessment.message;
  }

  return translateMessage(
    t,
    "project_shared.repository_access.copy.pending",
    "Repository access has not been checked yet.",
  );
}

export function formatRepositoryAccessSummary(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
  t?: Translate,
) {
  if (!repositoryUrl.trim()) {
    return translateMessage(
      t,
      "project_shared.repository_access.summary.pending",
      "Pending",
    );
  }

  if (isAssessingRepositoryAccess) {
    return translateMessage(
      t,
      "project_shared.repository_access.summary.checking",
      "Checking",
    );
  }

  if (repositoryAccessError) {
    return translateMessage(
      t,
      "project_shared.repository_access.summary.check_failed",
      "Check failed",
    );
  }

  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.summary.pending",
      "Pending",
    );
  }

  switch (assessment.visibility) {
    case "public":
      return translateMessage(
        t,
        "project_shared.repository_access.summary.public",
        "Public",
      );
    case "private":
      return translateMessage(
        t,
        "project_shared.repository_access.summary.private",
        "Private",
      );
    case "invalid":
      return translateMessage(
        t,
        "project_shared.repository_access.summary.invalid",
        "Invalid",
      );
    default:
      return translateMessage(
        t,
        "project_shared.repository_access.summary.unknown",
        "Unknown",
      );
  }
}

export function formatRepositoryAccessProviderLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
  t?: Translate,
) {
  if (isAssessingRepositoryAccess) {
    return translateMessage(
      t,
      "project_shared.repository_access.provider.detecting",
      "Detecting",
    );
  }

  if (repositoryAccessError || !assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.provider.pending",
      "Pending",
    );
  }

  return assessment.provider_label;
}

export function formatRepositoryVisibilityLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
  t?: Translate,
) {
  if (isAssessingRepositoryAccess) {
    return translateMessage(
      t,
      "project_shared.repository_access.visibility.checking",
      "Checking",
    );
  }

  if (repositoryAccessError) {
    return translateMessage(
      t,
      "project_shared.repository_access.visibility.needs_review",
      "Needs review",
    );
  }

  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.visibility.pending",
      "Pending",
    );
  }

  switch (assessment.visibility) {
    case "public":
      return translateMessage(
        t,
        "project_shared.repository_access.visibility.public",
        "Public",
      );
    case "private":
      return translateMessage(
        t,
        "project_shared.repository_access.visibility.private",
        "Private",
      );
    case "invalid":
      return translateMessage(
        t,
        "project_shared.repository_access.visibility.invalid",
        "Invalid",
      );
    default:
      return translateMessage(
        t,
        "project_shared.repository_access.visibility.unknown",
        "Unknown",
      );
  }
}

export function formatRepositoryLoginStatus(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
  t?: Translate,
) {
  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.login.pending",
      "Pending",
    );
  }

  if (assessment.auth_requirement === "none") {
    return translateMessage(
      t,
      "project_shared.repository_access.login.not_required",
      "Not required",
    );
  }

  if (!assessment.supports_interactive_login) {
    return translateMessage(
      t,
      "project_shared.repository_access.login.not_available",
      "Not available",
    );
  }

  if (assessment.provider_id === "github") {
    return formatGithubAuthProviderStatus(
      githubAuthProvider,
      isLoadingAuthProviders,
      t,
    );
  }

  return translateMessage(
    t,
    "project_shared.repository_access.login.required",
    "Required",
  );
}

export function formatRepositoryBindingStatus(
  assessment: RepositoryAccessAssessment | null,
  repositoryCredentialId: number | null,
  pendingRepositoryAccessAction: boolean,
  t?: Translate,
) {
  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.binding.pending",
      "Pending",
    );
  }

  if (pendingRepositoryAccessAction) {
    return translateMessage(
      t,
      "project_shared.repository_access.binding.connecting",
      "Connecting",
    );
  }

  if (assessment.auth_requirement === "none") {
    return translateMessage(
      t,
      "project_shared.repository_access.binding.not_required",
      "Not required",
    );
  }

  if (!supportsShellRepositoryLoginAction(assessment)) {
    return translateMessage(
      t,
      "project_shared.repository_access.binding.unavailable",
      "Unavailable",
    );
  }

  return repositoryCredentialId
    ? translateMessage(
        t,
        "project_shared.repository_access.binding.selected",
        "Selected",
      )
    : translateMessage(
        t,
        "project_shared.repository_access.binding.pending",
        "Pending",
      );
}

export function formatRepositoryBindingActionLabel(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  repositoryCredentialId: number | null,
  t?: Translate,
) {
  if (!assessment) {
    return translateMessage(
      t,
      "project_shared.repository_access.binding_action.connect_credential",
      "Connect credential",
    );
  }

  if (repositoryCredentialId) {
    return assessment.provider_id === "github"
      ? translateMessage(
          t,
          "project_shared.repository_access.binding_action.reconnect_github",
          "Reconnect GitHub login",
        )
      : translateMessage(
          t,
          "project_shared.repository_access.binding_action.change_credential",
          "Change credential",
        );
  }

  if (assessment.provider_id === "github") {
    return githubAuthProvider?.status === "connected"
      ? translateMessage(
          t,
          "project_shared.repository_access.binding_action.connect_github",
          "Connect GitHub login",
        )
      : translateMessage(
          t,
          "project_shared.repository_access.binding_action.login_and_connect",
          "Log in and connect",
        );
  }

  return translateMessage(
    t,
    "project_shared.repository_access.binding_action.select_credential",
    "Select credential",
  );
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
  t?: Translate,
) {
  if (isLoadingRepositoryCredentials) {
    return translateMessage(
      t,
      "project_shared.repository_access.credentials.loading",
      "Loading stored repository credentials...",
    );
  }

  if (!assessment || assessment.auth_requirement !== "required") {
    return translateMessage(
      t,
      "project_shared.repository_access.credentials.hint_public",
      "Public repositories can keep this empty.",
    );
  }

  return translateMessage(
    t,
    "project_shared.repository_access.credentials.hint_private",
    "Choose a stored GitHub credential or use the login action below.",
  );
}

export function buildRepositoryAccessAssessmentFromDetection(
  detection: RepositoryProviderDetection,
  repositoryVisibility: ProjectDraft["repositoryVisibility"],
  t?: Translate,
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
      message: translateMessage(
        t,
        "project_shared.repository_access.assessment.public",
        "Public repository selected. HGP will poll and clone this remote without repository authentication.",
      ),
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
      message: translateMessage(
        t,
        "project_shared.repository_access.assessment.private_github",
        "Private GitHub repository selected. Log in and connect this project before saving.",
      ),
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
      message: translateMessage(
        t,
        "project_shared.repository_access.assessment.private_unknown",
        "Private repository selected, but HGP could not identify a supported login platform from this URL. Only public repositories are supported for this host right now.",
      ),
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
    message: translateMessage(
      t,
      "project_shared.repository_access.assessment.private_provider",
      "Private {{providerLabel}} repositories are not supported yet. Only public repositories are available for this platform right now.",
      {
        providerLabel: detection.provider_label,
      },
    ),
  };
}

export function buildDetectedUnityEditorOptions(
  editors: DiscoveredUnityEditor[],
  isLoadingUnityAdapterSettings: boolean,
  unityAdapterSettingsError: string | null,
  t?: Translate,
): SelectOption[] {
  if (isLoadingUnityAdapterSettings) {
    return [
      {
        label: translateMessage(
          t,
          "project_shared.unity_editor.option.scanning",
          "Scanning installed Unity editors...",
        ),
        value: "",
      },
    ];
  }

  if (unityAdapterSettingsError) {
    return [
      {
        label: translateMessage(
          t,
          "project_shared.unity_editor.option.load_failed",
          "Unable to load installed Unity editors",
        ),
        value: "",
      },
    ];
  }

  if (editors.length === 0) {
    return [
      {
        label: translateMessage(
          t,
          "project_shared.unity_editor.option.none_detected",
          "No installed Unity editors detected",
        ),
        value: "",
      },
    ];
  }

  return [
    {
      label: translateMessage(
        t,
        "project_shared.unity_editor.option.select_detected",
        "Choose a detected Unity editor",
      ),
      title: translateMessage(
        t,
        "project_shared.unity_editor.option.select_detected",
        "Choose a detected Unity editor",
      ),
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
  t?: Translate,
) {
  if (unityAdapterSettingsError) {
    return translateMessage(
      t,
      "project_shared.unity_editor.hint.load_failed",
      "{{error}} Use the manual path field below to continue.",
      {
        error: unityAdapterSettingsError,
      },
    );
  }

  if (editorCount === 0) {
    return translateMessage(
      t,
      "project_shared.unity_editor.hint.none_detected",
      "Choose a detected editor when available, or keep using the manual executable path field below.",
    );
  }

  return translateMessage(
    t,
    "project_shared.unity_editor.hint.detected",
    "Select a detected Unity install to fill the executable path below, or keep using the manual picker.",
  );
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

export function normalizeBuildProcessPriority(
  value: string | null | undefined,
): BuildProcessPriority {
  switch ((value ?? "").trim()) {
    case "normal":
      return "normal";
    case "high":
      return "high";
    default:
      return "low";
  }
}

export function formatBuildProcessPriorityLabel(
  priority: BuildProcessPriority,
  t?: Translate,
) {
  switch (priority) {
    case "normal":
      return translateMessage(
        t,
        "project_shared.build_target.process_priority.normal",
        "Normal",
      );
    case "high":
      return translateMessage(
        t,
        "project_shared.build_target.process_priority.high",
        "High",
      );
    default:
      return translateMessage(
        t,
        "project_shared.build_target.process_priority.low",
        "Low",
      );
  }
}

export function formatBuildTargetExecutableSummary(
  diagnostics: UnityExecutableValidation | null,
  isValidating: boolean,
  t?: Translate,
) {
  if (isValidating) {
    return translateMessage(
      t,
      "project_shared.build_target.executable_summary.checking",
      "checking",
    );
  }

  if (!diagnostics) {
    return translateMessage(
      t,
      "project_shared.build_target.executable_summary.pending",
      "pending",
    );
  }

  return formatDiagnosticStatus(diagnostics.status, t);
}

export function buildBuildTargetQuickViewCopy(
  target: BuildTargetDraft,
  diagnostics: UnityExecutableValidation | null,
  unityExecutablePath: string,
  t?: Translate,
) {
  if (diagnostics && diagnostics.status !== "ready") {
    return diagnostics.message;
  }

  if (!unityExecutablePath.trim()) {
    return translateMessage(
      t,
      "project_shared.build_target.quick_view.executable_pending",
      "Unity executable path is still pending.",
    );
  }

  return `${
    target.buildMethod.trim() ||
    translateMessage(
      t,
      "project_shared.build_target.quick_view.build_method_pending",
      "Build method pending",
    )
  } • ${unityExecutablePath.trim()}`;
}

export function formatProjectSourceReviewDescription(
  draft: ProjectDraft,
  t?: Translate,
) {
  if (draft.projectKind === "repository") {
    return (
      draft.repositoryUrl.trim() ||
      translateMessage(
        t,
        "project_shared.source_review.repository_pending",
        "Repository source not set yet.",
      )
    );
  }

  return (
    draft.localPath.trim() ||
    translateMessage(
      t,
      "project_shared.source_review.local_pending",
      "Local workspace source not set yet.",
    )
  );
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
  const { t } = useLocalization();

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
            t,
          )}
        </Badge>
      }
      className="wizard-support-panel"
      description={resolveRepositoryAccessCopy(
        repositoryUrl,
        repositoryAccessAssessment,
        isAssessingRepositoryAccess,
        repositoryAccessError,
        t,
      )}
      eyebrow={t(
        "project_shared.repository_access.panel.eyebrow",
        "Repository",
      )}
      headerSeparated
      summary={
        repositoryUrl.trim() ||
        isAssessingRepositoryAccess ||
        repositoryAccessAssessment ||
        repositoryAccessError ? (
          <MetaRow className="wizard-callout__meta">
            <MetaItem
              label={t(
                "project_shared.repository_access.meta.provider",
                "Provider",
              )}
            >
              {formatRepositoryAccessProviderLabel(
                repositoryAccessAssessment,
                isAssessingRepositoryAccess,
                repositoryAccessError,
                t,
              )}
            </MetaItem>
            <MetaItem
              label={t(
                "project_shared.repository_access.meta.visibility",
                "Visibility",
              )}
            >
              {formatRepositoryVisibilityLabel(
                repositoryAccessAssessment,
                isAssessingRepositoryAccess,
                repositoryAccessError,
                t,
              )}
            </MetaItem>
            <MetaItem
              label={t("project_shared.repository_access.meta.login", "Login")}
            >
              {formatRepositoryLoginStatus(
                repositoryAccessAssessment,
                githubAuthProvider,
                isLoadingAuthProviders,
                t,
              )}
            </MetaItem>
            <MetaItem
              label={t(
                "project_shared.repository_access.meta.connection",
                "Connection",
              )}
            >
              {formatRepositoryBindingStatus(
                repositoryAccessAssessment,
                repositoryCredentialId,
                pendingRepositoryAccessAction,
                t,
              )}
            </MetaItem>
          </MetaRow>
        ) : undefined
      }
      title={t(
        "project_shared.repository_access.panel.title",
        "Repository access",
      )}
      tone="inset"
    >
      {repositoryAccessActionMessage ? (
        <p className="feed-banner feed-banner--info">
          {repositoryAccessActionMessage}
        </p>
      ) : null}

      {validationError ? (
        <p className="ui-field__error">{validationError}</p>
      ) : null}

      {repositoryAccessError ||
      authProviderError ||
      repositoryCredentialsError ? (
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
                ? t(
                    "project_shared.repository_access.actions.retrying_check",
                    "Retrying access check...",
                  )
                : t(
                    "project_shared.repository_access.actions.retry_check",
                    "Retry access check",
                  )}
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
                ? t(
                    "project_shared.repository_access.actions.retrying_accounts",
                    "Retrying accounts...",
                  )
                : t(
                    "project_shared.repository_access.actions.retry_accounts",
                    "Retry accounts",
                  )}
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
                ? t(
                    "project_shared.repository_access.actions.retrying_credentials",
                    "Retrying credentials...",
                  )
                : t(
                    "project_shared.repository_access.actions.retry_credentials",
                    "Retry credentials",
                  )}
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
              t,
            )}
            label={t(
              "project_shared.repository_access.credentials.label",
              "Repository credential",
            )}
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
                  ? t(
                      "project_shared.repository_access.actions.connecting_login",
                      "Connecting login...",
                    )
                  : formatRepositoryBindingActionLabel(
                      repositoryAccessAssessment,
                      githubAuthProvider,
                      repositoryCredentialId,
                      t,
                    )}
              </Button>
            ) : null}

            {repositoryCredentialId !== null &&
            onClearRepositoryAccessBinding ? (
              <Button
                onClick={onClearRepositoryAccessBinding}
                size="sm"
                variant="ghost"
              >
                {t(
                  "project_shared.repository_access.actions.disconnect",
                  "Disconnect",
                )}
              </Button>
            ) : null}

            {onManageAuth &&
            supportsShellRepositoryLoginAction(repositoryAccessAssessment) ? (
              <Button onClick={onManageAuth} size="sm" variant="ghost">
                {t(
                  "project_shared.repository_access.actions.open_accounts",
                  "Open accounts",
                )}
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
  const { t } = useLocalization();

  return (
    <div className="wizard-form-grid">
      <TextField
        error={errors?.name}
        label={t("project_shared.identity.name.label", "Project name")}
        onBlur={() => onFieldBlur?.("name")}
        onChange={(event) => onNameChange(event.currentTarget.value)}
        placeholder={t(
          "project_shared.identity.name.placeholder",
          "Red Horizon",
        )}
        value={draft.name}
      />

      <SelectField
        error={errors?.projectKind}
        label={t("project_shared.identity.kind.label", "Project kind")}
        onBlur={() => onFieldBlur?.("projectKind")}
        onChange={(event) =>
          onProjectKindChange(
            event.currentTarget.value as ProjectDraft["projectKind"],
          )
        }
        options={buildProjectKindOptions(t)}
        value={draft.projectKind}
      />

      <RepositoryEngineField
        error={errors?.engineKind}
        label={t("project_shared.identity.engine.label", "Engine")}
        onBlur={() => onFieldBlur?.("engineKind")}
        onChange={(event) =>
          onEngineKindChange(event.currentTarget.value as RepositoryEngineKind)
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
  const { t } = useLocalization();

  return (
    <>
      <div className="wizard-form-grid">
        <TextField
          error={repositoryUrlError}
          hint={t(
            "project_shared.access.repository_url.hint",
            "Use the HTTPS remote that HGP will poll and clone.",
          )}
          label={t(
            "project_shared.access.repository_url.label",
            "Repository URL",
          )}
          leadingIcon="server"
          onBlur={onRepositoryUrlBlur}
          onChange={(event) => onRepositoryUrlChange(event.currentTarget.value)}
          placeholder={t(
            "project_shared.access.repository_url.placeholder",
            "https://github.com/org/project.git",
          )}
          value={repositoryUrl}
        />

        <SelectField
          hint={t(
            "project_shared.access.repository_visibility.hint",
            "Tell HGP whether this remote should be treated as public or private.",
          )}
          label={t(
            "project_shared.access.repository_visibility.label",
            "Repository visibility",
          )}
          onBlur={onRepositoryVisibilityBlur}
          onChange={(event) =>
            onRepositoryVisibilityChange(
              event.currentTarget.value as ProjectDraft["repositoryVisibility"],
            )
          }
          options={buildRepositoryVisibilityOptions(t)}
          value={repositoryVisibility}
        />

        <TextField
          error={pollingIntervalSecondsError}
          hint={t(
            "project_shared.access.polling_interval.hint",
            "Polling stays operator-visible. The runtime requires at least 5 seconds.",
          )}
          label={t(
            "project_shared.access.polling_interval.label",
            "Polling interval (seconds)",
          )}
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
  const { t } = useLocalization();

  return (
    <div className="wizard-form-grid">
      <PathPickerField
        buttonLabel={t("project_shared.local_path.button", "Choose workspace")}
        clearable
        disabled={disabled}
        dialogTitle={t(
          "project_shared.local_path.dialog_title",
          "Select local workspace directory",
        )}
        error={localPathError}
        hint={t(
          "project_shared.local_path.hint",
          "Choose the host-local Unity workspace that HGP should build directly.",
        )}
        label={t("project_shared.local_path.label", "Local workspace path")}
        onClear={onClearLocalPath}
        onError={onPathPickError}
        onPathPicked={onPathPicked}
        pickerKind="directory"
        placeholder={t(
          "project_shared.local_path.placeholder",
          "C:/projects/red-horizon",
        )}
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
  const { t } = useLocalization();

  if (buildTargetAdapter.kind !== "unity") {
    return (
      <div className="wizard-form-grid">
        {rootError ? (
          <p className="feed-banner feed-banner--error">{rootError}</p>
        ) : null}

        <div className="wizard-callout wizard-callout--compact">
          <p className="wizard-callout__copy">
            {buildTargetAdapter.unsupportedMessage ??
              buildTargetAdapter.supportCopy}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="wizard-targets-shell">
      {rootError ? (
        <p className="feed-banner feed-banner--error">{rootError}</p>
      ) : null}
      {removalCallout}

      <SelectField
        disabled={detectedEditorDisabled}
        hint={detectedEditorHint}
        label={t(
          "project_shared.targets.detected_editors.label",
          "Installed Unity editors",
        )}
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
        buttonLabel={t(
          "project_shared.targets.unity_executable.button",
          "Choose Unity executable",
        )}
        dialogTitle={t(
          "project_shared.targets.unity_executable.dialog_title",
          "Select Unity Editor executable",
        )}
        error={unityExecutableError}
        filters={[
          {
            extensions: ["exe", "app"],
            name: t(
              "project_shared.targets.unity_executable.filter_name",
              "Unity Editor",
            ),
          },
        ]}
        hint={t(
          "project_shared.targets.unity_executable.hint",
          "Select the host-local Unity Editor executable that should run every build target in this project.",
        )}
        label={t(
          "project_shared.targets.unity_executable.label",
          "Unity executable",
        )}
        onError={onUnityExecutablePickError}
        onPathPicked={onUnityExecutablePicked}
        pickerKind="file"
        placeholder={t(
          "project_shared.targets.unity_executable.placeholder",
          "C:/Program Files/Unity/Hub/Editor/.../Unity.exe",
        )}
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
          {t(
            "project_shared.targets.unity_executable.validating",
            "Validating Unity executable path...",
          )}
        </p>
      ) : null}

      {buildTargets.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "project_shared.targets.empty.title",
              "No build targets configured.",
            )}
          </p>
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
                  {t("project_shared.targets.actions.edit", "Edit")}
                </Button>
                <Button
                  disabled={isBusy}
                  leadingIcon="trash"
                  onClick={() => onRemoveTarget(target.id)}
                  size="sm"
                  variant="ghost"
                >
                  {t("project_shared.targets.actions.remove", "Remove")}
                </Button>
              </div>
            }
            className="publish-destination-quick-view"
            key={target.id}
            summary={
              <MetaRow className="wizard-target-card__summary">
                <MetaItem
                  label={t(
                    "project_shared.targets.summary.platform",
                    "Platform",
                  )}
                >
                  {target.targetPlatform.trim() ||
                    t("project_shared.targets.summary.pending", "pending")}
                </MetaItem>
                <MetaItem
                  label={t(
                    "project_shared.targets.summary.build_method",
                    "Build method",
                  )}
                >
                  {target.buildMethod.trim() ||
                    t("project_shared.targets.summary.pending", "pending")}
                </MetaItem>
                <MetaItem
                  label={t(
                    "project_shared.targets.summary.unity_executable",
                    "Unity executable",
                  )}
                >
                  {formatBuildTargetExecutableSummary(
                    unityExecutableDiagnostics,
                    isValidatingUnityExecutable,
                    t,
                  )}
                </MetaItem>
              </MetaRow>
            }
            title={
              target.name.trim() ||
              t(
                "project_shared.targets.card.default_name",
                "Build target {{index}}",
                { index: index + 1 },
              )
            }
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
                  t,
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
          {t("project_shared.targets.actions.add", "Add target")}
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
  editingMode?: PublishDestinationEditingMode;
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
  editingMode = "inline",
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
      editingMode={editingMode}
      errors={errors}
      onChange={onChange}
      onSaveCredential={onSaveCredential}
      showItchUserversionTemplate={showItchUserversionTemplate}
    />
  );
}

type ProjectPathsStepProps = {
  workspaceRootOverride: string;
  workspaceRootOverrideError?: string;
  disabled?: boolean;
  onWorkspaceRootClear: () => void;
  onPathPickError: (error: unknown) => void;
  onWorkspaceRootPicked: (selectedPath: string) => void;
};

export function ProjectPathsStep({
  workspaceRootOverride,
  workspaceRootOverrideError,
  disabled = false,
  onWorkspaceRootClear,
  onPathPickError,
  onWorkspaceRootPicked,
}: ProjectPathsStepProps) {
  const { t } = useLocalization();

  return (
    <div className="wizard-form-grid">
      <PathPickerField
        buttonLabel={t(
          "project_shared.paths.workspace.button",
          "Choose workspace root",
        )}
        clearable
        clearLabel={t(
          "project_shared.paths.workspace.clear_label",
          "Use default",
        )}
        disabled={disabled}
        dialogTitle={t(
          "project_shared.paths.workspace.dialog_title",
          "Select managed workspace root directory",
        )}
        error={workspaceRootOverrideError}
        hint={t(
          "project_shared.paths.workspace.hint",
          "Defaults to the host user folder under HGPWorkspaces/<project-name>. Choose another root only when this project must live elsewhere.",
        )}
        label={t(
          "project_shared.paths.workspace.label",
          "Workspace root override",
        )}
        onClear={onWorkspaceRootClear}
        onError={onPathPickError}
        onPathPicked={onWorkspaceRootPicked}
        pickerKind="directory"
        placeholder={t(
          "project_shared.paths.workspace.placeholder",
          "C:/Users/operator/HGPWorkspaces/Red Horizon",
        )}
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
  const { t } = useLocalization();
  const reviewBuildTargets = draft.buildTargets.map((target) => ({
    id: target.id,
    buildTargetId: target.buildTargetId ?? null,
    name:
      target.name.trim() ||
      t("project_shared.targets.review.unnamed_target", "Unnamed target"),
  }));
  const publishDestinationReviewSummary = buildPublishDestinationReviewSummary(
    draft.publishDestinations,
    reviewBuildTargets,
  ).map((destination) => {
    const draftDestination = draft.publishDestinations.find(
      (entry) => entry.id === destination.id,
    );

    return {
      ...destination,
      kindLabel:
        draftDestination?.kind === "filesystem"
          ? t("publish_destinations.editor.kind.folder", "Folder")
          : draftDestination?.kind === "itch"
            ? t("publish_destinations.editor.kind.itch", "Itch")
            : destination.kindLabel,
    };
  });
  const unboundPublishTargetNames = listUnboundBuildTargetNames(
    draft.publishDestinations,
    reviewBuildTargets,
  );

  return (
    <div className="wizard-review-shell">
      <SurfacePanel
        className="wizard-review-panel"
        description={formatProjectSourceReviewDescription(draft, t)}
        eyebrow={t("project_shared.review.project.eyebrow", "Project")}
        headerSeparated
        summary={
          <MetaRow>
            <MetaItem
              label={t("project_shared.review.project.engine", "Engine")}
            >
              {formatRepositoryEngineKindLabel(draft.engineKind, t)}
            </MetaItem>
            {draft.projectKind === "repository" ? (
              <MetaItem label={t("project_shared.review.project.poll", "Poll")}>
                {`${draft.pollingIntervalSeconds.trim() || "0"}s`}
              </MetaItem>
            ) : (
              <MetaItem
                label={t("project_shared.review.project.source", "Source")}
              >
                {t(
                  "project_shared.review.project.no_remote_polling",
                  "No remote polling",
                )}
              </MetaItem>
            )}
            <MetaItem
              label={
                draft.projectKind === "repository"
                  ? t("project_shared.review.project.access", "Access")
                  : t("project_shared.review.project.source", "Source")
              }
            >
              {draft.projectKind === "repository"
                ? repositoryAccessSummary
                : formatProjectSourceAdapterStatus(projectSourceStepAdapter, t)}
            </MetaItem>
          </MetaRow>
        }
        title={
          draft.name.trim() ||
          t("project_shared.review.project.unnamed", "Unnamed project")
        }
        tone="inset"
      >
        <p className="wizard-summary-panel__copy">
          {t(
            "project_shared.review.project.copy",
            "{{projectKind}} with {{targetCount}} configured for registration.",
            {
              projectKind: formatProjectKindLabel(draft.projectKind, t),
              targetCount: formatWizardTargetCount(
                draft.buildTargets.length,
                t,
              ),
            },
          )}
        </p>
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description={buildTargetAdapter.reviewDescription}
        eyebrow={t("project_shared.review.targets.eyebrow", "Build Targets")}
        headerSeparated
        title={t("project_shared.review.targets.title", "Target Review")}
        tone="inset"
      >
        {buildTargetAdapter.kind === "unity" ? (
          <div className="wizard-summary-list">
            {draft.buildTargets.map((target) => (
              <div className="wizard-summary-list__item" key={target.id}>
                <div className="wizard-summary-list__title-row">
                  <strong>
                    {target.name.trim() ||
                      t(
                        "project_shared.targets.review.unnamed_target",
                        "Unnamed target",
                      )}
                  </strong>
                  <Badge tone="neutral">
                    {target.targetPlatform ||
                      t(
                        "project_shared.review.targets.target_platform_pending",
                        "Unity target pending",
                      )}
                  </Badge>
                </div>
                <p className="wizard-summary-list__copy">
                  {target.buildMethod.trim() ||
                    t(
                      "project_shared.review.targets.build_method_pending",
                      "Unity build method pending",
                    )}
                </p>
              </div>
            ))}
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>
                  {t(
                    "project_shared.review.targets.shared_executable",
                    "Shared Unity executable",
                  )}
                </strong>
                <Badge tone="muted">
                  {formatBuildTargetExecutableSummary(
                    unityExecutableDiagnostics,
                    isValidatingUnityExecutable,
                    t,
                  )}
                </Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                {draft.unityExecutablePath.trim() ||
                  t(
                    "project_shared.review.targets.executable_pending",
                    "Unity executable pending",
                  )}
              </p>
            </div>
          </div>
        ) : (
          <div className="wizard-summary-list">
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>{buildTargetAdapter.supportTitle}</strong>
                <Badge tone="muted">
                  {t(
                    "project_shared.review.targets.unavailable",
                    "unavailable",
                  )}
                </Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                {buildTargetAdapter.unsupportedMessage ??
                  buildTargetAdapter.supportCopy}
              </p>
            </div>
          </div>
        )}
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description={t(
          "project_shared.review.publish.description",
          "Destination-specific publish bindings and credential readiness.",
        )}
        eyebrow={t(
          "project_shared.review.publish.eyebrow",
          "Publish Destinations",
        )}
        headerSeparated
        title={t("project_shared.review.publish.title", "Destination Review")}
        tone="inset"
      >
        <div className="wizard-summary-list">
          {publishDestinationReviewSummary.length === 0 ? (
            <div className="wizard-summary-list__item">
              <div className="wizard-summary-list__title-row">
                <strong>
                  {t(
                    "project_shared.review.publish.empty.title",
                    "No publish destinations configured",
                  )}
                </strong>
                <Badge tone="muted">
                  {t("project_shared.review.publish.empty.status", "valid")}
                </Badge>
              </div>
              <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                {t(
                  "project_shared.review.publish.empty.copy",
                  "Every build target will keep its artifact under the runtime-managed output root.",
                )}
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
                    : t(
                        "project_shared.review.publish.destination.no_targets",
                        "No build targets bound yet.",
                      )}
                </p>
                <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                  {destination.missingCredential
                    ? t(
                        "project_shared.review.publish.destination.credential_missing",
                        "Credential still missing.",
                      )
                    : t(
                        "project_shared.review.publish.destination.copy",
                        "Uploads are managed automatically by HGP for the selected channels.",
                      )}
                </p>
              </div>
            ))
          )}

          <div className="wizard-summary-list__item">
            <div className="wizard-summary-list__title-row">
              <strong>
                {t(
                  "project_shared.review.publish.unbound.title",
                  "Unbound build targets",
                )}
              </strong>
              <Badge tone="muted">
                {unboundPublishTargetNames.length === 0
                  ? t("project_shared.review.publish.unbound.none", "none")
                  : t(
                      "project_shared.review.publish.unbound.kept_local",
                      "kept local",
                    )}
              </Badge>
            </div>
            <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
              {unboundPublishTargetNames.length > 0
                ? unboundPublishTargetNames.join(", ")
                : t(
                    "project_shared.review.publish.unbound.copy",
                    "Every configured build target is bound to at least one publish destination.",
                  )}
            </p>
          </div>
        </div>
      </SurfacePanel>

      <SurfacePanel
        className="wizard-review-panel"
        description={t(
          "project_shared.review.paths.description",
          "Managed workspace path that HGP will use for this project.",
        )}
        eyebrow={t("project_shared.review.paths.eyebrow", "Paths")}
        headerSeparated
        title={t("project_shared.review.paths.title", "Path Review")}
        tone="inset"
      >
        <div className="wizard-summary-list">
          <div className="wizard-summary-list__item">
            <div className="wizard-summary-list__title-row">
              <strong>
                {t(
                  "project_shared.review.paths.workspace.title",
                  "Workspace root",
                )}
              </strong>
            </div>
            <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
              {draft.workspaceRootOverride.trim() ||
                t(
                  "project_shared.review.paths.workspace.default_copy",
                  "Use the default HGPWorkspaces root under the host user folder.",
                )}
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
