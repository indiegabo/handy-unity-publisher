import { useEffect, useId, useRef } from "react";

import { IconButton } from "./Button";

export type FullScreenModalProps = {
  title?: string;
  description?: string;
  children?: React.ReactNode;
  dismissible?: boolean;
  className?: string;
  onResolve?: (value?: any) => void;
};

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

const PROGRAMMATIC_FOCUS_SELECTOR = [
  FOCUSABLE_SELECTOR,
  '[tabindex="-1"]',
].join(", ");

const FullScreenModal = ({
  title,
  description,
  children,
  dismissible = true,
  className,
  onResolve,
}: FullScreenModalProps) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();

  useEffect(() => {
    const container = containerRef.current;

    if (!container) {
      return;
    }

    const focusableNodes = getFocusableNodes(container);
    const preferredFocusTarget = resolvePreferredFocusTarget(
      container,
      focusableNodes,
    );
    const nextFocusTarget =
      preferredFocusTarget ?? focusableNodes[0] ?? container;

    nextFocusTarget.focus();
  }, []);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && dismissible) {
      event.preventDefault();
      event.stopPropagation();
      onResolve?.(null);
      return;
    }

    if (event.key !== "Tab") {
      return;
    }

    const container = containerRef.current;

    if (!container) {
      return;
    }

    const focusableNodes = getFocusableNodes(container);

    if (focusableNodes.length === 0) {
      event.preventDefault();
      container.focus();
      return;
    }

    const firstNode = focusableNodes[0];
    const lastNode = focusableNodes[focusableNodes.length - 1];
    const activeElement = document.activeElement;

    if (event.shiftKey && activeElement === firstNode) {
      event.preventDefault();
      lastNode.focus();
      return;
    }

    if (!event.shiftKey && activeElement === lastNode) {
      event.preventDefault();
      firstNode.focus();
    }
  };

  return (
    <div
      aria-modal="true"
      aria-describedby={description ? descriptionId : undefined}
      aria-labelledby={title ? titleId : undefined}
      className={joinClassNames("full-screen-modal", className)}
      onKeyDown={handleKeyDown}
      ref={containerRef}
      role="dialog"
      tabIndex={-1}
    >
      <header className="full-screen-modal__header">
        <div className="full-screen-modal__title-block">
          {title ? (
            <h2 className="full-screen-modal__title" id={titleId}>
              {title}
            </h2>
          ) : null}
          {description ? (
            <p className="full-screen-modal__description" id={descriptionId}>
              {description}
            </p>
          ) : null}
        </div>

        {dismissible ? (
          <IconButton
            className="full-screen-modal__close"
            icon="close"
            label="Close overlay"
            onClick={() => onResolve?.(null)}
            size="sm"
            variant="ghost"
          />
        ) : null}
      </header>

      <div className="full-screen-modal__body">{children}</div>
    </div>
  );
};

export default FullScreenModal;

function getFocusableNodes(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  );
}

function resolvePreferredFocusTarget(
  container: HTMLElement,
  focusableNodes: HTMLElement[],
) {
  const preferredFocusTargets = Array.from(
    container.querySelectorAll<HTMLElement>(
      '[data-overlay-autofocus]:not([data-overlay-autofocus="false"])',
    ),
  );

  return preferredFocusTargets.find(
    (candidate) =>
      candidate.matches(PROGRAMMATIC_FOCUS_SELECTOR) ||
      focusableNodes.some((node) => node === candidate),
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
