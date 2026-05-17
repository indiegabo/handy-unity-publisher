import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useId,
  useState,
} from "react";

import { Icon } from "./Icon";

export type AccordionTriggerMode = "both" | "button" | "header";
export type AccordionTone = "default" | "section";

type VerticalAccordionProps = {
  animatedBorder?: boolean;
  bodyClassName?: string;
  bodyInset?: boolean;
  children: ReactNode;
  className?: string;
  collapsedToggleLabel?: string;
  defaultOpen?: boolean;
  expandedToggleLabel?: string;
  header: ReactNode;
  headerClassName?: string;
  headerSeparated?: boolean;
  onOpenChange?: (nextOpen: boolean) => void;
  open?: boolean;
  tone?: AccordionTone;
  triggerMode?: AccordionTriggerMode;
};

export function VerticalAccordion({
  animatedBorder = false,
  bodyClassName,
  bodyInset = false,
  children,
  className,
  collapsedToggleLabel = "Expand section",
  defaultOpen = false,
  expandedToggleLabel = "Collapse section",
  header,
  headerClassName,
  headerSeparated = false,
  onOpenChange,
  open,
  tone = "default",
  triggerMode = "both",
}: VerticalAccordionProps) {
  const [uncontrolledIsOpen, setUncontrolledIsOpen] = useState(defaultOpen);
  const bodyId = useId();
  const headerIsInteractive =
    triggerMode === "both" || triggerMode === "header";
  const isControlled = open !== undefined;
  const isOpen = isControlled ? open : uncontrolledIsOpen;

  const toggle = () => {
    const nextOpen = !isOpen;

    if (!isControlled) {
      setUncontrolledIsOpen(nextOpen);
    }

    onOpenChange?.(nextOpen);
  };

  const handleHeaderClick = () => {
    if (!headerIsInteractive) {
      return;
    }

    toggle();
  };

  const handleHeaderKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!headerIsInteractive) {
      return;
    }

    if (event.key !== "Enter" && event.key !== " ") {
      return;
    }

    event.preventDefault();
    toggle();
  };

  const handleToggleClick = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    toggle();
  };

  return (
    <div
      className={joinClassNames(
        "vertical-accordion",
        animatedBorder && "vertical-accordion--animated-border",
        bodyInset && "vertical-accordion--body-inset",
        headerSeparated && "vertical-accordion--header-separated",
        isOpen && "vertical-accordion--open",
        `vertical-accordion--${tone}`,
        className,
      )}
      data-state={isOpen ? "open" : "closed"}
    >
      <div
        aria-controls={headerIsInteractive ? bodyId : undefined}
        aria-expanded={headerIsInteractive ? isOpen : undefined}
        className={joinClassNames(
          "vertical-accordion__header",
          headerIsInteractive && "vertical-accordion__header--interactive",
          headerClassName,
        )}
        onClick={handleHeaderClick}
        onKeyDown={handleHeaderKeyDown}
        role={headerIsInteractive ? "button" : undefined}
        tabIndex={headerIsInteractive ? 0 : undefined}
      >
        <button
          aria-controls={bodyId}
          aria-expanded={isOpen}
          aria-label={isOpen ? expandedToggleLabel : collapsedToggleLabel}
          className="vertical-accordion__toggle"
          onClick={handleToggleClick}
          title={isOpen ? expandedToggleLabel : collapsedToggleLabel}
          type="button"
        >
          <Icon
            aria-hidden
            className="ui-button__icon"
            name="chevronDown"
            style={{ transform: isOpen ? "rotate(0deg)" : "rotate(-90deg)" }}
          />
        </button>

        <div className="vertical-accordion__header-content">{header}</div>
      </div>

      <div
        aria-hidden={!isOpen}
        className={joinClassNames("vertical-accordion__body", bodyClassName)}
        hidden={!isOpen}
        id={bodyId}
        style={{ display: isOpen ? "grid" : "none" }}
      >
        <div className="vertical-accordion__body-inner">{children}</div>
      </div>
    </div>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
