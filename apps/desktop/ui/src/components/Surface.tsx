import type { HTMLAttributes, ReactNode } from "react";

export type BadgeTone = "muted" | "neutral" | "strong";
export type SurfacePanelTone = "section" | "inset" | "ghost";

type SurfacePanelProps = Omit<HTMLAttributes<HTMLElement>, "title"> & {
  actions?: ReactNode;
  bodyClassName?: string;
  description?: string;
  eyebrow?: string;
  headerClassName?: string;
  headerSeparated?: boolean;
  summary?: ReactNode;
  title: string;
  tone?: SurfacePanelTone;
};

type FocusPageFrameProps = Omit<HTMLAttributes<HTMLElement>, "title"> & {
  actions?: ReactNode;
  bodyClassName?: string;
  description?: string;
  eyebrow?: string;
  summary?: ReactNode;
  title: string;
};

type SummaryStripProps = HTMLAttributes<HTMLDivElement>;

type MetaRowProps = HTMLAttributes<HTMLDivElement>;

type MetaItemProps = Omit<HTMLAttributes<HTMLSpanElement>, "children"> & {
  children: ReactNode;
  label?: ReactNode;
};

type BadgeProps = {
  children: ReactNode;
  className?: string;
  tone?: BadgeTone;
};

export function FocusPageFrame({
  actions,
  bodyClassName,
  children,
  className,
  description,
  eyebrow,
  summary,
  title,
  ...props
}: FocusPageFrameProps) {
  return (
    <section {...props} className={joinClassNames("focus-page-frame", className)}>
      <header className="focus-page-frame__header">
        <div className="focus-page-frame__title-block">
          {eyebrow ? <p className="focus-page-frame__eyebrow">{eyebrow}</p> : null}
          <h1 className="focus-page-frame__title">{title}</h1>
          {description ? (
            <p className="focus-page-frame__description">{description}</p>
          ) : null}
        </div>
        {actions ? (
          <div className="focus-page-frame__actions">{actions}</div>
        ) : null}
      </header>
      {summary ? (
        <SummaryStrip className="focus-page-frame__summary">{summary}</SummaryStrip>
      ) : null}
      <div className={joinClassNames("focus-page-frame__body", bodyClassName)}>
        {children}
      </div>
    </section>
  );
}

export function SurfacePanel({
  actions,
  bodyClassName,
  children,
  className,
  description,
  eyebrow,
  headerClassName,
  headerSeparated = false,
  summary,
  title,
  tone = "section",
  ...props
}: SurfacePanelProps) {
  return (
    <section
      {...props}
      className={joinClassNames(
        "ui-panel",
        `ui-panel--${tone}`,
        headerSeparated && "ui-panel--header-separated",
        className,
      )}
    >
      <header className={joinClassNames("ui-panel__header", headerClassName)}>
        <div className="ui-panel__title-block">
          {eyebrow ? <p className="ui-panel__eyebrow">{eyebrow}</p> : null}
          <h2 className="ui-panel__title">{title}</h2>
          {description ? <p className="ui-panel__description">{description}</p> : null}
        </div>
        {actions ? <div className="ui-panel__actions">{actions}</div> : null}
      </header>
      {summary ? <SummaryStrip className="ui-panel__summary">{summary}</SummaryStrip> : null}
      <div className={joinClassNames("ui-panel__body", bodyClassName)}>
        {children}
      </div>
    </section>
  );
}

export function SummaryStrip({
  children,
  className,
  ...props
}: SummaryStripProps) {
  return (
    <div {...props} className={joinClassNames("ui-summary-strip", className)}>
      {children}
    </div>
  );
}

export function MetaRow({ children, className, ...props }: MetaRowProps) {
  return (
    <div {...props} className={joinClassNames("ui-meta-row", className)}>
      {children}
    </div>
  );
}

export function MetaItem({
  children,
  className,
  label,
  ...props
}: MetaItemProps) {
  return (
    <span {...props} className={joinClassNames("ui-meta-item", className)}>
      {label ? <span className="ui-meta-item__label">{label}</span> : null}
      <span className="ui-meta-item__value">{children}</span>
    </span>
  );
}

export function Badge({ children, className, tone = "neutral" }: BadgeProps) {
  return (
    <span className={joinClassNames("ui-badge", `ui-badge--${tone}`, className)}>
      {children}
    </span>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}