import { invoke } from "@tauri-apps/api/core";
import {
    subscribeToRuntimeEvents,
    type RuntimeEventRecord,
} from "./runtimeEvents";

const PROCESS_FEED_EVENT_TOPICS = new Set<string>([
    "automation.release_queued",
    "build.run_started",
    "build.run_finished",
    "build.run_on_hold",
    "build.stage_updated",
    "publish.run_started",
    "publish.run_finished",
]);

export type ProcessFeedRuntimeEvent = RuntimeEventRecord;

export async function subscribeToProcessFeedEvents(
    listener: (event: ProcessFeedRuntimeEvent) => void,
): Promise<() => void> {
    return subscribeToRuntimeEvents((event) => {
        if (!PROCESS_FEED_EVENT_TOPICS.has(event.topic)) {
            return;
        }

        listener(event);
    });
}

type NotifyProcessOnHoldInput = {
    releaseRunId: number;
    repositoryName: string;
    gitTag: string;
    reason: string | null;
};

export async function notifyProcessOnHold(
    input: NotifyProcessOnHoldInput,
): Promise<void> {
    await invoke("notify_process_on_hold", {
        input: {
            release_run_id: input.releaseRunId,
            repository_name: input.repositoryName,
            git_tag: input.gitTag,
            reason: input.reason,
        },
    });
}