import type { BadgeTone } from "./Surface";

export type ProcessFeedTranslate = (
  key: string,
  fallback: string,
  values?: Record<string, string | number>,
) => string;

export type ProcessFeedRecord = {
  release_run_id: number;
  repository_id: number;
  repository_name: string;
  repository_url: string | null;
  repository_engine_kind: string;
  git_tag: string;
  git_commit: string | null;
  engine_version: string | null;
  display_status: string;
  current_step_label: string;
  current_step_status: string;
  current_step_detail: string | null;
  queued_build_runs: number;
  running_build_runs: number;
  succeeded_build_runs: number;
  failed_build_runs: number;
  canceled_build_runs: number;
  queued_publish_runs: number;
  running_publish_runs: number;
  succeeded_publish_runs: number;
  failed_publish_runs: number;
  canceled_publish_runs: number;
  total_build_runs: number;
  total_publish_runs: number;
  started_at: string | null;
  finished_at: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
};

export function resolveProcessFeedStepLabel(
  process: ProcessFeedRecord,
  status: string,
) {
  return (
    process.current_step_label.trim() ||
    process.current_step_detail?.trim() ||
    buildFallbackStep(status)
  );
}

export function resolveLocalizedProcessFeedStepLabel(
  translate: ProcessFeedTranslate,
  process: ProcessFeedRecord,
  status: string,
) {
  return (
    process.current_step_label.trim() ||
    process.current_step_detail?.trim() ||
    buildLocalizedFallbackStep(translate, status)
  );
}

export function resolveProcessFeedStepDetail(process: ProcessFeedRecord) {
  const detail = process.current_step_detail?.trim();
  if (!detail) {
    return null;
  }

  if (detail === process.current_step_label.trim()) {
    return null;
  }

  return detail;
}

export function formatProcessFeedEngineVersionBadge(engineVersion: string | null) {
  if (engineVersion?.trim()) {
    return `Engine ${engineVersion.trim()}`;
  }

  return "Engine pending";
}

export function formatLocalizedProcessFeedEngineVersionBadge(
  translate: ProcessFeedTranslate,
  engineVersion: string | null,
) {
  if (engineVersion?.trim()) {
    return translate(
      "process_feed.badges.engine_version",
      "Engine {{engineVersion}}",
      {
        engineVersion: engineVersion.trim(),
      },
    );
  }

  return translate("process_feed.badges.engine_pending", "Engine pending");
}

export function formatProcessFeedEngineKindBadge(engineKind: string) {
  const normalized = engineKind.trim();
  if (!normalized) {
    return "engine: unknown";
  }

  return `engine: ${normalized}`;
}

export function formatLocalizedProcessFeedEngineKindBadge(
  translate: ProcessFeedTranslate,
  engineKind: string,
) {
  const normalized = engineKind.trim();
  if (!normalized) {
    return translate(
      "process_feed.badges.engine_kind_unknown",
      "engine: unknown",
    );
  }

  return translate(
    "process_feed.badges.engine_kind",
    "engine: {{engineKind}}",
    {
      engineKind: normalized,
    },
  );
}

export function formatProcessFeedBuildCount(totalBuildRuns: number) {
  if (totalBuildRuns === 1) {
    return "1 build";
  }

  return `${totalBuildRuns} builds`;
}

export function formatLocalizedProcessFeedBuildCount(
  translate: ProcessFeedTranslate,
  totalBuildRuns: number,
) {
  if (totalBuildRuns === 1) {
    return translate("process_feed.count.build.one", "1 build");
  }

  return translate(
    "process_feed.count.build.other",
    "{{count}} builds",
    {
      count: totalBuildRuns,
    },
  );
}

export function formatProcessFeedPublishCount(totalPublishRuns: number) {
  if (totalPublishRuns === 1) {
    return "1 publish";
  }

  return `${totalPublishRuns} publishes`;
}

export function formatLocalizedProcessFeedPublishCount(
  translate: ProcessFeedTranslate,
  totalPublishRuns: number,
) {
  if (totalPublishRuns === 1) {
    return translate("process_feed.count.publish.one", "1 publish");
  }

  return translate(
    "process_feed.count.publish.other",
    "{{count}} publishes",
    {
      count: totalPublishRuns,
    },
  );
}

export function normalizeProcessFeedDisplayStatus(status: string) {
  switch (status) {
    case "queued":
    case "running":
    case "on_hold":
    case "succeeded":
    case "failed":
    case "canceled":
      return status;
    default:
      return "queued";
  }
}

export function resolveProcessFeedStatusTone(status: string): BadgeTone {
  switch (normalizeProcessFeedDisplayStatus(status)) {
    case "succeeded":
      return "strong";
    case "on_hold":
      return "neutral";
    case "failed":
    case "canceled":
      return "neutral";
    default:
      return "muted";
  }
}

export function formatProcessFeedStatusLabel(status: string) {
  const normalized = normalizeProcessFeedDisplayStatus(status).replace(/_/g, " ");
  return normalized.charAt(0).toUpperCase() + normalized.slice(1);
}

export function formatLocalizedProcessFeedStatusLabel(
  translate: ProcessFeedTranslate,
  status: string,
) {
  switch (normalizeProcessFeedDisplayStatus(status)) {
    case "queued":
      return translate("process_feed.status.queued", "Queued");
    case "running":
      return translate("process_feed.status.running", "Running");
    case "on_hold":
      return translate("process_feed.status.on_hold", "On hold");
    case "succeeded":
      return translate("process_feed.status.succeeded", "Succeeded");
    case "failed":
      return translate("process_feed.status.failed", "Failed");
    case "canceled":
      return translate("process_feed.status.canceled", "Canceled");
  }
}

export function formatProcessFeedMetaValue(
  value: string | null,
  emptyLabel = "pending",
) {
  return value?.trim() || emptyLabel;
}

export function formatLocalizedProcessFeedMetaValue(
  translate: ProcessFeedTranslate,
  value: string | null,
  emptyKey = "process_feed.meta.pending",
  emptyFallback = "pending",
) {
  return value?.trim() || translate(emptyKey, emptyFallback);
}

function buildFallbackStep(status: string) {
  switch (status) {
    case "queued":
      return "The runtime is still planning this process.";
    case "running":
      return "The runtime is still updating this process.";
    case "on_hold":
      return "This process is on hold until the local workspace lock is released.";
    case "succeeded":
      return "All recorded work for this process finished cleanly.";
    case "failed":
      return "At least one build or publish task failed.";
    case "canceled":
      return "The process stopped before every child task finished.";
    default:
      return "The runtime is still planning this process.";
  }
}

function buildLocalizedFallbackStep(
  translate: ProcessFeedTranslate,
  status: string,
) {
  switch (status) {
    case "queued":
      return translate(
        "process_feed.fallback_step.queued",
        "The runtime is still planning this process.",
      );
    case "running":
      return translate(
        "process_feed.fallback_step.running",
        "The runtime is still updating this process.",
      );
    case "on_hold":
      return translate(
        "process_feed.fallback_step.on_hold",
        "This process is on hold until the local workspace lock is released.",
      );
    case "succeeded":
      return translate(
        "process_feed.fallback_step.succeeded",
        "All recorded work for this process finished cleanly.",
      );
    case "failed":
      return translate(
        "process_feed.fallback_step.failed",
        "At least one build or publish task failed.",
      );
    case "canceled":
      return translate(
        "process_feed.fallback_step.canceled",
        "The process stopped before every child task finished.",
      );
    default:
      return translate(
        "process_feed.fallback_step.queued",
        "The runtime is still planning this process.",
      );
  }
}