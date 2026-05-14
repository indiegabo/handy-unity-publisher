import { useEffectEvent, useState } from "react";

import { Button, IconButton } from "./Button";
import {
  pickHostPath,
  type HostPathSelectionKind,
  type PickHostPathFilter,
  type PickHostPathInput,
} from "../services/projects";

type PathPickerFieldProps = {
  buttonLabel: string;
  clearable?: boolean;
  clearLabel?: string;
  dialogTitle?: string;
  disabled?: boolean;
  error?: string;
  filters?: PickHostPathFilter[];
  hint?: string;
  label: string;
  onClear?: () => void;
  onError?: (error: unknown) => void;
  onPathPicked: (path: string) => void;
  pickerKind: HostPathSelectionKind;
  placeholder?: string;
  value: string;
};

export function PathPickerField({
  buttonLabel,
  clearable = false,
  clearLabel = "Clear",
  dialogTitle,
  disabled = false,
  error,
  filters,
  hint,
  label,
  onClear,
  onError,
  onPathPicked,
  pickerKind,
  placeholder,
  value,
}: PathPickerFieldProps) {
  const [isPicking, setIsPicking] = useState(false);
  const canClear = clearable && Boolean(onClear) && Boolean(value.trim());

  const handlePick = useEffectEvent(async () => {
    if (disabled || isPicking) {
      return;
    }

    setIsPicking(true);

    try {
      const input: PickHostPathInput = {
        kind: pickerKind,
      };

      if (dialogTitle?.trim()) {
        input.title = dialogTitle.trim();
      }

      if (filters && filters.length > 0) {
        input.filters = filters;
      }

      const selectedPath = await pickHostPath(input);
      if (selectedPath) {
        onPathPicked(selectedPath);
      }
    } catch (pickError) {
      onError?.(pickError);
    } finally {
      setIsPicking(false);
    }
  });

  return (
    <label className={joinClassNames("ui-field", error && "ui-field--invalid")}>
      <span className="ui-field__header">
        <span className="ui-field__label">{label}</span>
        {hint ? <span className="ui-field__hint">{hint}</span> : null}
      </span>

      <span className="path-picker-field__row">
        <span className="path-picker-field__field">
          <input
            className="ui-field__input"
            disabled
            placeholder={placeholder}
            value={value}
          />
        </span>

        <span className="path-picker-field__actions">
          <IconButton
            disabled={disabled || isPicking}
            icon="folder"
            label={buttonLabel}
            onClick={() => {
              void handlePick();
            }}
            size="sm"
            variant="secondary"
          />

          {canClear ? (
            <Button
              disabled={disabled || isPicking}
              onClick={onClear}
              size="sm"
              variant="ghost"
            >
              {clearLabel}
            </Button>
          ) : null}
        </span>
      </span>

      {error ? <span className="ui-field__error">{error}</span> : null}
    </label>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
