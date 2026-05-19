import type { ComponentType, InputHTMLAttributes } from "react";

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
        <span className="ui-field__label">{label}</span>
        {hint ? <span className="ui-field__hint">{hint}</span> : null}
      </span>

      <span className="input-with-picker__row">
        <span className="input-with-picker__field">
          <span className="ui-field__control">
            {leadingIcon ? (
              <Icon className="ui-field__icon" name={leadingIcon} />
            ) : null}
            <input
              {...inputProps}
              autoComplete={autoComplete}
              className={joinClassNames(
                "ui-field__input",
                leadingIcon && "ui-field__input--with-icon",
              )}
              disabled={disabled}
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

      {error ? <span className="ui-field__error">{error}</span> : null}
    </label>
  );
};

export default InputWithPicker;

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
