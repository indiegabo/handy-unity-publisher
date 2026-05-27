import { useDeferredValue, useMemo, useRef, useState } from "react";

import { Button } from "./Button";
import { TextField } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { MetaItem, MetaRow, SummaryStrip } from "./Surface";
import { useLocalization } from "../LocalizationProvider";

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
  initialValues?: readonly string[];
  items?: readonly SelectListItem[];
  selectionLabel?: string;
  selectionMode?: "single" | "multiple";
  submitLabel?: string;
  title?: string;
  onResolve?: (value?: string | string[] | null) => void;
};

const SelectListFullScreen = ({
  description,
  emptyStateCopy,
  emptyStateTitle,
  initialQuery,
  initialValue,
  initialValues,
  items = [],
  selectionLabel,
  selectionMode = "single",
  submitLabel,
  title,
  onResolve,
}: SelectListFullScreenProps) => {
  const { t } = useLocalization();
  const [query, setQuery] = useState(initialQuery ?? initialValue ?? "");
  const [selectedIds, setSelectedIds] = useState<string[]>(() => {
    if (selectionMode === "multiple") {
      return initialValues ? [...initialValues] : [];
    }

    return initialValue ? [initialValue] : [];
  });
  const deferredQuery = useDeferredValue(query);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const resolvedDescription =
    description ??
    t(
      "select_list.description",
      "Search the available inventory and select a single result to continue.",
    );
  const resolvedEmptyStateCopy =
    emptyStateCopy ??
    t(
      "select_list.empty.copy",
      "Try a broader filter or close the overlay and adjust the source list.",
    );
  const resolvedEmptyStateTitle =
    emptyStateTitle ??
    t(
      "select_list.empty.title",
      "No results matched the current filter.",
    );
  const resolvedSelectionLabel =
    selectionLabel ?? t("select_list.action.selected", "Selected");
  const resolvedSelectLabel = t("select_list.action.select", "Select");
  const resolvedSubmitLabel =
    submitLabel ?? t("select_list.actions.submit", "Apply selection");
  const resolvedTitle =
    title ?? t("select_list.title", "Select item");

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
  const resultCountHint =
    filtered.length === 1
      ? t("select_list.results.one", "1 result")
      : t("select_list.results.other", "{{count}} results", {
          count: filtered.length,
        });

  const handleSelectItem = (itemId: string) => {
    if (selectionMode === "multiple") {
      setSelectedIds((current) =>
        current.includes(itemId)
          ? current.filter((existingId) => existingId !== itemId)
          : [...current, itemId],
      );
      return;
    }

    onResolve?.(itemId);
  };

  return (
    <FullScreenModal
      description={resolvedDescription}
      onResolve={onResolve}
      title={resolvedTitle}
    >
      <div className="select-list-modal">
        <TextField
          autoComplete="off"
          data-overlay-autofocus
          hint={resultCountHint}
          label={t("select_list.filter.label", "Filter inventory")}
          leadingIcon="search"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && filtered.length > 0) {
              event.preventDefault();
              itemRefs.current[0]?.focus();
            }
          }}
          placeholder={t(
            "select_list.filter.placeholder",
            "Search by name or secondary text",
          )}
          value={query}
        />

        {selectionMode === "multiple" ? (
          <SummaryStrip className="select-list-modal__summary-strip">
            <MetaRow className="select-list-modal__summary">
              <MetaItem
                label={t("select_list.summary.selected", "Selected")}
              >
                {selectedIds.length}
              </MetaItem>
              <MetaItem label={t("select_list.summary.results", "Results")}>
                {filtered.length}
              </MetaItem>
            </MetaRow>
          </SummaryStrip>
        ) : null}

        <div
          aria-label={t("select_list.results.aria_label", "Selectable results")}
          className="select-list-modal__list"
          role="list"
        >
          {filtered.length === 0 ? (
            <div className="feed-state select-list-modal__empty">
              <p className="feed-state__title">{resolvedEmptyStateTitle}</p>
              <p className="feed-state__copy">{resolvedEmptyStateCopy}</p>
            </div>
          ) : (
            <>
              {filtered.map((item, index) => (
                <button
                  aria-pressed={
                    selectionMode === "multiple"
                      ? selectedIdSet.has(item.id)
                      : undefined
                  }
                  className={joinClassNames(
                    "select-list-modal__item",
                    selectionMode === "multiple" &&
                      selectedIdSet.has(item.id) &&
                      "select-list-modal__item--selected",
                  )}
                  key={item.id}
                  onClick={() => handleSelectItem(item.id)}
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
                  <span className="select-list-modal__item-action">
                    {selectionMode === "multiple"
                      ? selectedIdSet.has(item.id)
                        ? resolvedSelectionLabel
                        : resolvedSelectLabel
                      : resolvedSelectLabel}
                  </span>
                </button>
              ))}
            </>
          )}
        </div>

        {selectionMode === "multiple" ? (
          <div className="select-list-modal__actions">
            <Button
              disabled={selectedIds.length === 0}
              onClick={() => setSelectedIds([])}
              size="sm"
              variant="ghost"
            >
              {t("select_list.actions.clear", "Clear selection")}
            </Button>
            <Button
              disabled={selectedIds.length === 0}
              onClick={() => onResolve?.(selectedIds)}
              size="sm"
              variant="primary"
            >
              {resolvedSubmitLabel}
            </Button>
          </div>
        ) : null}
      </div>
    </FullScreenModal>
  );
};

export default SelectListFullScreen;

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
