import { type SelectOption } from "../Field";
import { type Translate } from "../../LocalizationProvider";
import { type ProcessPriority } from "../../services/projects";

export function normalizeProcessPriority(value: string): ProcessPriority {
    switch (value.trim()) {
        case "normal":
            return "normal";
        case "high":
            return "high";
        default:
            return "low";
    }
}

export function buildProcessPriorityOptions(t: Translate): SelectOption[] {
    return [
        {
            label: t("project_shared.build_target.process_priority.low", "Low"),
            value: "low",
        },
        {
            label: t(
                "project_shared.build_target.process_priority.normal",
                "Normal",
            ),
            value: "normal",
        },
        {
            label: t("project_shared.build_target.process_priority.high", "High"),
            value: "high",
        },
    ];
}