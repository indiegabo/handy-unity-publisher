import type {
  InputHTMLAttributes,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";
import { useId } from "react";

import { Icon, type IconName } from "./Icon";

export type SelectOption = {
  disabled?: boolean;
  label: string;
  title?: string;
  value: string;
};

export type SelectOptionGroup = {
  label: string;
  options: readonly SelectOption[];
};

type SelectFieldOption = SelectOption | SelectOptionGroup;

type FieldBaseProps = {
  error?: string;
  hint?: string;
  label: string;
};

export type TextFieldProps = FieldBaseProps &
  Omit<InputHTMLAttributes<HTMLInputElement>, "size"> & {
    leadingIcon?: IconName;
  };

export type SelectFieldProps = FieldBaseProps &
  Omit<SelectHTMLAttributes<HTMLSelectElement>, "children"> & {
    options: readonly SelectFieldOption[];
  };

export type TextAreaFieldProps = FieldBaseProps &
  TextareaHTMLAttributes<HTMLTextAreaElement>;

export function TextField({
  "aria-describedby": ariaDescribedBy,
  "aria-labelledby": ariaLabelledBy,
  className,
  error,
  hint,
  id,
  label,
  leadingIcon,
  ...props
}: TextFieldProps) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const labelId = `${fieldId}-label`;
  const hintId = hint ? `${fieldId}-hint` : undefined;
  const errorId = error ? `${fieldId}-error` : undefined;

  return (
    <label
      className={joinClassNames(
        "ui-field",
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
      <span className="ui-field__control">
        {leadingIcon ? (
          <Icon className="ui-field__icon" name={leadingIcon} />
        ) : null}
        <input
          {...props}
          aria-describedby={joinAriaReferences(
            ariaDescribedBy,
            hintId,
            errorId,
          )}
          aria-labelledby={joinAriaReferences(ariaLabelledBy, labelId)}
          className={joinClassNames(
            "ui-field__input",
            leadingIcon && "ui-field__input--with-icon",
          )}
          id={fieldId}
        />
      </span>
      {error ? (
        <span className="ui-field__error" id={errorId}>
          {error}
        </span>
      ) : null}
    </label>
  );
}

export function SelectField({
  "aria-describedby": ariaDescribedBy,
  "aria-labelledby": ariaLabelledBy,
  className,
  error,
  hint,
  id,
  label,
  options,
  ...props
}: SelectFieldProps) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const labelId = `${fieldId}-label`;
  const hintId = hint ? `${fieldId}-hint` : undefined;
  const errorId = error ? `${fieldId}-error` : undefined;
  const selectedOptionTitle = findSelectOptionByValue(
    options,
    String(props.value ?? ""),
  )?.title;

  return (
    <label
      className={joinClassNames(
        "ui-field",
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
      <span className="ui-field__control">
        <select
          {...props}
          aria-describedby={joinAriaReferences(
            ariaDescribedBy,
            hintId,
            errorId,
          )}
          aria-labelledby={joinAriaReferences(ariaLabelledBy, labelId)}
          className="ui-field__select"
          id={fieldId}
          title={props.title ?? selectedOptionTitle}
        >
          {options.map((option, index) =>
            isSelectOptionGroup(option) ? (
              <optgroup key={`${option.label}-${index}`} label={option.label}>
                {option.options.map(renderSelectOption)}
              </optgroup>
            ) : (
              renderSelectOption(option)
            ),
          )}
        </select>
        <Icon className="ui-field__chevron" name="chevronDown" size={14} />
      </span>
      {error ? (
        <span className="ui-field__error" id={errorId}>
          {error}
        </span>
      ) : null}
    </label>
  );
}

export function TextAreaField({
  "aria-describedby": ariaDescribedBy,
  "aria-labelledby": ariaLabelledBy,
  className,
  error,
  hint,
  id,
  label,
  ...props
}: TextAreaFieldProps) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const labelId = `${fieldId}-label`;
  const hintId = hint ? `${fieldId}-hint` : undefined;
  const errorId = error ? `${fieldId}-error` : undefined;

  return (
    <label
      className={joinClassNames(
        "ui-field",
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
      <span className="ui-field__control ui-field__control--textarea">
        <textarea
          {...props}
          aria-describedby={joinAriaReferences(
            ariaDescribedBy,
            hintId,
            errorId,
          )}
          aria-labelledby={joinAriaReferences(ariaLabelledBy, labelId)}
          className="ui-field__textarea"
          id={fieldId}
        />
      </span>
      {error ? (
        <span className="ui-field__error" id={errorId}>
          {error}
        </span>
      ) : null}
    </label>
  );
}

function joinAriaReferences(
  ...references: Array<string | undefined>
): string | undefined {
  const tokens = references
    .flatMap((reference) => reference?.split(/\s+/) ?? [])
    .map((reference) => reference.trim())
    .filter(Boolean);

  return tokens.length > 0 ? tokens.join(" ") : undefined;
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}

function renderSelectOption(option: SelectOption) {
  return (
    <option
      disabled={option.disabled}
      key={option.value}
      title={option.title}
      value={option.value}
    >
      {option.label}
    </option>
  );
}

function isSelectOptionGroup(
  option: SelectFieldOption,
): option is SelectOptionGroup {
  return "options" in option;
}

function findSelectOptionByValue(
  options: readonly SelectFieldOption[],
  value: string,
): SelectOption | null {
  for (const option of options) {
    if (isSelectOptionGroup(option)) {
      const match = option.options.find((entry) => entry.value === value);

      if (match) {
        return match;
      }

      continue;
    }

    if (option.value === value) {
      return option;
    }
  }

  return null;
}
