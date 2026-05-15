import type { ButtonHTMLAttributes } from "react";

export type WorkerStatusTone = "success" | "warning" | "idle";

type WorkerStatusIndicatorProps = Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "children"
> & {
  animated?: boolean;
  className?: string;
  expanded?: boolean;
  label: string;
  tone: WorkerStatusTone;
};

export function WorkerStatusIndicator({
  animated = false,
  className,
  expanded = false,
  label,
  tone,
  type = "button",
  ...props
}: WorkerStatusIndicatorProps) {
  return (
    <button
      {...props}
      aria-label={label}
      className={joinClassNames(
        "worker-status-indicator",
        `worker-status-indicator--${tone}`,
        animated && "worker-status-indicator--animated",
        expanded && "worker-status-indicator--expanded",
        className,
      )}
      title={label}
      type={type}
    >
      <span aria-hidden="true" className="worker-status-indicator__halo" />
      <span aria-hidden="true" className="worker-status-indicator__core" />
    </button>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
