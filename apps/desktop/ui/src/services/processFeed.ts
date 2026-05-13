import {
    subscribeToRuntimeEvents,
    type RuntimeEventRecord,
} from "./runtimeEvents";

const PROCESS_FEED_EVENT_TOPICS = new Set<string>([
    "automation.release_queued",
    "build.run_started",
    "build.run_finished",
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