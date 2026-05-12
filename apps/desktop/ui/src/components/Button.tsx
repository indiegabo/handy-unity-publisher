import type { ButtonHTMLAttributes } from "react";

import { Icon, type IconName } from "./Icon";

export type ButtonVariant = "ghost" | "primary" | "secondary";
export type ButtonSize = "sm" | "md";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  iconOnly?: boolean;
  leadingIcon?: IconName;
  size?: ButtonSize;
  trailingIcon?: IconName;
  variant?: ButtonVariant;
};

export type IconButtonProps = Omit<
  ButtonProps,
  "children" | "iconOnly" | "leadingIcon" | "trailingIcon"
> & {
  icon: IconName;
  label: string;
};

export function Button({
  children,
  className,
  iconOnly = false,
  leadingIcon,
  size = "md",
  trailingIcon,
  type = "button",
  variant = "secondary",
  ...props
}: ButtonProps) {
  return (
    <button
      {...props}
      className={joinClassNames(
        "ui-button",
        `ui-button--${variant}`,
        `ui-button--${size}`,
        iconOnly && "ui-button--icon-only",
        className,
      )}
      type={type}
    >
      {leadingIcon ? <Icon className="ui-button__icon" name={leadingIcon} /> : null}
      {children ? <span className="ui-button__label">{children}</span> : null}
      {trailingIcon ? <Icon className="ui-button__icon" name={trailingIcon} /> : null}
    </button>
  );
}

export function IconButton({ icon, label, ...props }: IconButtonProps) {
  return (
    <Button
      {...props}
      aria-label={label}
      iconOnly
      leadingIcon={icon}
      title={label}
    />
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}