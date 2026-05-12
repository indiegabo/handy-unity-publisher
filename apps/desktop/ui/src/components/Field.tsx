import type {
  InputHTMLAttributes,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";

import { Icon, type IconName } from "./Icon";

export type SelectOption = {
  disabled?: boolean;
  label: string;
  value: string;
};

type FieldBaseProps = {
  hint?: string;
  label: string;
};

export type TextFieldProps = FieldBaseProps &
  Omit<InputHTMLAttributes<HTMLInputElement>, "size"> & {
    leadingIcon?: IconName;
  };

export type SelectFieldProps = FieldBaseProps &
  Omit<SelectHTMLAttributes<HTMLSelectElement>, "children"> & {
    options: readonly SelectOption[];
  };

export type TextAreaFieldProps = FieldBaseProps &
  TextareaHTMLAttributes<HTMLTextAreaElement>;

export function TextField({
  className,
  hint,
  label,
  leadingIcon,
  ...props
}: TextFieldProps) {
  return (
    <label className={joinClassNames("ui-field", className)}>
      <span className="ui-field__header">
        <span className="ui-field__label">{label}</span>
        {hint ? <span className="ui-field__hint">{hint}</span> : null}
      </span>
      <span className="ui-field__control">
        {leadingIcon ? <Icon className="ui-field__icon" name={leadingIcon} /> : null}
        <input
          {...props}
          className={joinClassNames(
            "ui-field__input",
            leadingIcon && "ui-field__input--with-icon",
          )}
        />
      </span>
    </label>
  );
}

export function SelectField({
  className,
  hint,
  label,
  options,
  ...props
}: SelectFieldProps) {
  return (
    <label className={joinClassNames("ui-field", className)}>
      <span className="ui-field__header">
        <span className="ui-field__label">{label}</span>
        {hint ? <span className="ui-field__hint">{hint}</span> : null}
      </span>
      <span className="ui-field__control">
        <select {...props} className="ui-field__select">
          {options.map((option) => (
            <option disabled={option.disabled} key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <Icon className="ui-field__chevron" name="chevronDown" size={14} />
      </span>
    </label>
  );
}

export function TextAreaField({
  className,
  hint,
  label,
  ...props
}: TextAreaFieldProps) {
  return (
    <label className={joinClassNames("ui-field", className)}>
      <span className="ui-field__header">
        <span className="ui-field__label">{label}</span>
        {hint ? <span className="ui-field__hint">{hint}</span> : null}
      </span>
      <span className="ui-field__control ui-field__control--textarea">
        <textarea {...props} className="ui-field__textarea" />
      </span>
    </label>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}