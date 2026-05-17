import type { BadgeTone } from "./Surface";

export type ProcessFeedRecord = {
  release_run_id: number;
  repository_id: number;
  repository_name: string;
  repository_url: string;
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

export function formatProcessFeedEngineKindBadge(engineKind: string) {
  const normalized = engineKind.trim();
  if (!normalized) {
    return "engine: unknown";
  }

  return `engine: ${normalized}`;
}

export function formatProcessFeedBuildCount(totalBuildRuns: number) {
  if (totalBuildRuns === 1) {
    return "1 build";
  }

  return `${totalBuildRuns} builds`;
}

export function formatProcessFeedPublishCount(totalPublishRuns: number) {
  if (totalPublishRuns === 1) {
    return "1 publish";
  }

  return `${totalPublishRuns} publishes`;
}

export function normalizeProcessFeedDisplayStatus(status: string) {
  switch (status) {
    case "queued":
    case "running":
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

export function formatProcessFeedMetaValue(
  value: string | null,
  emptyLabel = "pending",
) {
  return value?.trim() || emptyLabel;
}

function buildFallbackStep(status: string) {
  switch (status) {
    case "queued":
      return "The runtime is still planning this process.";
    case "running":
      return "The runtime is still updating this process.";
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