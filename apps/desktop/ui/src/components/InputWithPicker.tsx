import {
  useId,
  type ComponentType,
  type InputHTMLAttributes,
  type Ref,
} from "react";

import { Button } from "./Button";
import { Icon, type IconName } from "./Icon";
import { useOverlay } from "./OverlayManager";

export type InputWithPickerProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "onChange" | "size" | "value"
> & {
  buttonIcon?: IconName;
  buttonLabel?: string;
  className?: string;
  disabled?: boolean;
  error?: string;
  hint?: string;
  inputRef?: Ref<HTMLInputElement>;
  leadingIcon?: IconName;
  onPick?: (value: string) => void;
  value?: string;
  onChange?: (value: string) => void;
  placeholder?: string;
  label: string;
  pickerComponent?: ComponentType<any>;
  pickerProps?: Record<string, unknown>;
};

const InputWithPicker = ({
  autoComplete,
  buttonIcon = "search",
  buttonLabel = "Browse",
  className,
  disabled = false,
  error,
  hint,
  inputRef,
  leadingIcon,
  onPick,
  onBlur,
  value,
  onChange,
  placeholder,
  label,
  pickerComponent,
  pickerProps,
  ...inputProps
}: InputWithPickerProps) => {
  const { openOverlay } = useOverlay();
  const inputId = useId();
  const labelId = `${inputId}-label`;
  const hintId = hint ? `${inputId}-hint` : undefined;
  const errorId = error ? `${inputId}-error` : undefined;
  const describedBy = [hintId, errorId].filter(Boolean).join(" ") || undefined;

  const handlePick = async () => {
    if (disabled || !pickerComponent) {
      return;
    }

    const result = await openOverlay<string>(pickerComponent, {
      initialValue: value,
      ...(pickerProps ?? {}),
    });

    if (result !== null && result !== undefined) {
      onPick?.(result);

      if (!onPick) {
        onChange?.(result);
      }
    }
  };

  return (
    <label
      className={joinClassNames(
        "ui-field",
        "input-with-picker",
        error && "ui-field--invalid",
        className,
      )}
    >
      <span className="ui-field__header">
        <span className="ui-field__label" id={labelId}>
          {label}
        </span>
        {hint ? (
          <span className="ui-field__hint" id={hintId}>
            {hint}
          </span>
        ) : null}
      </span>

      <span className="input-with-picker__row">
        <span className="input-with-picker__field">
          <span className="ui-field__control">
            {leadingIcon ? (
              <Icon className="ui-field__icon" name={leadingIcon} />
            ) : null}
            <input
              {...inputProps}
              aria-describedby={describedBy}
              aria-labelledby={labelId}
              autoComplete={autoComplete}
              className={joinClassNames(
                "ui-field__input",
                leadingIcon && "ui-field__input--with-icon",
              )}
              disabled={disabled}
              id={inputId}
              ref={inputRef}
              onBlur={onBlur}
              onChange={(event) => onChange?.(event.target.value)}
              placeholder={placeholder}
              value={value ?? ""}
            />
          </span>
        </span>

        <span className="input-with-picker__actions">
          <Button
            className="input-with-picker__button"
            disabled={disabled || !pickerComponent}
            leadingIcon={buttonIcon}
            onClick={() => {
              void handlePick();
            }}
            size="sm"
            variant="secondary"
          >
            {buttonLabel}
          </Button>
        </span>
      </span>

      {error ? (
        <span className="ui-field__error" id={errorId}>
          {error}
        </span>
      ) : null}
    </label>
  );
};

export default InputWithPicker;

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
