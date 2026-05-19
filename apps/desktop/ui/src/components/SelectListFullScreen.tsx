import { useDeferredValue, useMemo, useRef, useState } from "react";

import { TextField } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { MetaItem, MetaRow, SurfacePanel } from "./Surface";

export type SelectListItem = {
  id: string;
  label: string;
  subtitle?: string;
};

export type SelectListFullScreenProps = {
  description?: string;
  emptyStateCopy?: string;
  emptyStateTitle?: string;
  initialQuery?: string;
  initialValue?: string;
  items?: readonly SelectListItem[];
  title?: string;
  onResolve?: (value?: string | null) => void;
};

const SelectListFullScreen = ({
  description = "Search the available inventory and select a single result to continue.",
  emptyStateCopy = "Try a broader filter or close the overlay and adjust the source list.",
  emptyStateTitle = "No results matched the current filter.",
  initialQuery,
  initialValue,
  items = [],
  title = "Select item",
  onResolve,
}: SelectListFullScreenProps) => {
  const [query, setQuery] = useState(initialQuery ?? initialValue ?? "");
  const deferredQuery = useDeferredValue(query);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const filtered = useMemo(() => {
    const normalizedQuery = deferredQuery.trim().toLowerCase();

    if (!normalizedQuery) {
      return items;
    }

    return items.filter((item) => {
      return (
        item.label.toLowerCase().includes(normalizedQuery) ||
        (item.subtitle ?? "").toLowerCase().includes(normalizedQuery)
      );
    });
  }, [deferredQuery, items]);

  return (
    <FullScreenModal
      description={description}
      onResolve={onResolve}
      title={title}
    >
      <div className="select-list-modal">
        <TextField
          autoComplete="off"
          data-overlay-autofocus
          hint={`${filtered.length} result${filtered.length === 1 ? "" : "s"}`}
          label="Filter inventory"
          leadingIcon="search"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && filtered.length > 0) {
              event.preventDefault();
              itemRefs.current[0]?.focus();
            }
          }}
          placeholder="Search by name or secondary text"
          value={query}
        />

        <MetaRow className="select-list-modal__summary">
          <MetaItem label="Items">{items.length}</MetaItem>
          <MetaItem label="Visible">{filtered.length}</MetaItem>
          {deferredQuery.trim() ? (
            <MetaItem label="Query">{deferredQuery.trim()}</MetaItem>
          ) : null}
        </MetaRow>

        <SurfacePanel
          bodyClassName="select-list-modal__panel-body"
          description="Use the filter above to narrow the list, then choose a single result."
          title="Results"
          tone="inset"
        >
          {filtered.length === 0 ? (
            <div className="feed-state select-list-modal__empty">
              <p className="feed-state__title">{emptyStateTitle}</p>
              <p className="feed-state__copy">{emptyStateCopy}</p>
            </div>
          ) : (
            <div
              aria-label="Selectable results"
              className="select-list-modal__list"
              role="list"
            >
              {filtered.map((item, index) => (
                <button
                  className="select-list-modal__item"
                  key={item.id}
                  onClick={() => onResolve?.(item.id)}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowDown") {
                      event.preventDefault();
                      itemRefs.current[
                        Math.min(index + 1, filtered.length - 1)
                      ]?.focus();
                      return;
                    }

                    if (event.key === "ArrowUp") {
                      event.preventDefault();

                      if (index === 0) {
                        const container =
                          event.currentTarget.closest(".full-screen-modal");
                        const input = container?.querySelector<HTMLElement>(
                          "[data-overlay-autofocus]",
                        );
                        input?.focus();
                        return;
                      }

                      itemRefs.current[index - 1]?.focus();
                      return;
                    }

                    if (event.key === "Home") {
                      event.preventDefault();
                      itemRefs.current[0]?.focus();
                      return;
                    }

                    if (event.key === "End") {
                      event.preventDefault();
                      itemRefs.current[filtered.length - 1]?.focus();
                    }
                  }}
                  ref={(element) => {
                    itemRefs.current[index] = element;
                  }}
                  type="button"
                >
                  <span className="select-list-modal__item-content">
                    <span className="select-list-modal__item-label">
                      {item.label}
                    </span>
                    {item.subtitle ? (
                      <span className="select-list-modal__item-copy">
                        {item.subtitle}
                      </span>
                    ) : null}
                  </span>
                  <span className="select-list-modal__item-action">Select</span>
                </button>
              ))}
            </div>
          )}
        </SurfacePanel>
      </div>
    </FullScreenModal>
  );
};

export default SelectListFullScreen;
