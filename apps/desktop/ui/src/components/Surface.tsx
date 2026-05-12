import type { HTMLAttributes, ReactNode } from "react";

export type BadgeTone = "muted" | "neutral" | "strong";

type SurfacePanelProps = Omit<HTMLAttributes<HTMLElement>, "title"> & {
  actions?: ReactNode;
  description?: string;
  eyebrow?: string;
  title: string;
};

type BadgeProps = {
  children: ReactNode;
  className?: string;
  tone?: BadgeTone;
};

export function SurfacePanel({
  actions,
  children,
  className,
  description,
  eyebrow,
  title,
  ...props
}: SurfacePanelProps) {
  return (
    <section {...props} className={joinClassNames("ui-panel", className)}>
      <header className="ui-panel__header">
        <div className="ui-panel__title-block">
          {eyebrow ? <p className="ui-panel__eyebrow">{eyebrow}</p> : null}
          <h2 className="ui-panel__title">{title}</h2>
          {description ? <p className="ui-panel__description">{description}</p> : null}
        </div>
        {actions ? <div className="ui-panel__actions">{actions}</div> : null}
      </header>
      <div className="ui-panel__body">{children}</div>
    </section>
  );
}

export function Badge({ children, className, tone = "neutral" }: BadgeProps) {
  return <span className={joinClassNames("ui-badge", `ui-badge--${tone}`, className)}>{children}</span>;
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}