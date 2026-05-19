import type { ReactNode } from "react";

import { FocusPageFrame } from "./Surface";

export type ScreenScaffoldProps = {
  title?: string;
  subtitle?: string;
  actions?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
};

const ScreenScaffold = ({
  title,
  subtitle,
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
