import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type RuntimeEventRecord = {
    event_id: string;
    occurred_at_unix_millis: number;
    topic: string;
    severity: string;
    origin: string;
    user_requested: boolean;
    repository_id: number | null;
    release_run_id: number | null;
    build_run_id: number | null;
    publish_run_id: number | null;
    summary: string;
    payload: Record<string, unknown>;
};

export async function subscribeToRuntimeEvents(
    listener: (event: RuntimeEventRecord) => void,
): Promise<UnlistenFn> {
    return listen<RuntimeEventRecord>("runtime:event", (event) => {
        listener(event.payload);
    });
}

export function buildProcessElapsedRuntimeEventTopic(releaseRunId: number) {
    return `process.${releaseRunId}.elapsed_time`;
}

export async function subscribeToProcessElapsedClock(
    releaseRunId: number,
    listener: (elapsedClock: string) => void,
): Promise<UnlistenFn> {
    const topic = buildProcessElapsedRuntimeEventTopic(releaseRunId);

    return subscribeToRuntimeEvents((event) => {
        if (event.topic !== topic || event.release_run_id !== releaseRunId) {
            return;
        }

        const elapsedClock = event.payload.elapsed_clock;
        if (typeof elapsedClock !== "string" || !elapsedClock.trim()) {
            return;
        }

        listener(elapsedClock);
    });
}