import type { ReactNode } from "react";

import { FocusPageFrame } from "./Surface";

export type ScreenScaffoldProps = {
  eyebrow?: string;
  title?: string;
  subtitle?: string;
  summary?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
};

const ScreenScaffold = ({
  eyebrow,
  title,
  subtitle,
  summary,
  actions,
  children,
  footer,
  className,
}: ScreenScaffoldProps) => {
  return (
    <FocusPageFrame
      actions={actions}
      bodyClassName="screen-scaffold__body"
      className={joinClassNames("screen-scaffold", className)}
      description={subtitle}
      eyebrow={eyebrow}
      summary={summary}
      title={title ?? ""}
    >
      {children}
      {footer ? (
        <footer className="screen-scaffold__footer">{footer}</footer>
      ) : null}
    </FocusPageFrame>
  );
};

export default ScreenScaffold;

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
