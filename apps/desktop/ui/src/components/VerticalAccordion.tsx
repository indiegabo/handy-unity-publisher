import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useId,
  useState,
} from "react";

import { IconButton } from "./Button";

export type AccordionTriggerMode = "both" | "button" | "header";

type VerticalAccordionProps = {
  animatedBorder?: boolean;
  bodyClassName?: string;
  children: ReactNode;
  className?: string;
  collapsedToggleLabel?: string;
  defaultOpen?: boolean;
  expandedToggleLabel?: string;
  header: ReactNode;
  headerClassName?: string;
  onOpenChange?: (nextOpen: boolean) => void;
  open?: boolean;
  triggerMode?: AccordionTriggerMode;
};

export function VerticalAccordion({
  animatedBorder = false,
  bodyClassName,
  children,
  className,
  collapsedToggleLabel = "Expand section",
  defaultOpen = false,
  expandedToggleLabel = "Collapse section",
  header,
  headerClassName,
  onOpenChange,
  open,
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
        isOpen && "vertical-accordion--open",
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
        <IconButton
          aria-controls={bodyId}
          aria-expanded={isOpen}
          className="vertical-accordion__toggle"
          icon="chevronDown"
          label={isOpen ? expandedToggleLabel : collapsedToggleLabel}
          onClick={handleToggleClick}
          size="sm"
          variant="ghost"
        />

        <div className="vertical-accordion__header-content">{header}</div>
      </div>

      <div
        aria-hidden={!isOpen}
        className={joinClassNames("vertical-accordion__body", bodyClassName)}
        id={bodyId}
      >
        <div className="vertical-accordion__body-inner">{children}</div>
      </div>
    </div>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
