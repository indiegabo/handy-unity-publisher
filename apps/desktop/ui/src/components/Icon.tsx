import type { SVGProps } from "react";

export type IconName =
  | "arrowUpRight"
  | "box"
  | "chevronDown"
  | "folder"
  | "layout"
  | "play"
  | "plus"
  | "refresh"
  | "search"
  | "server"
  | "settings"
  | "spark"
  | "terminal";

type IconProps = Omit<SVGProps<SVGSVGElement>, "children"> & {
  name: IconName;
  decorative?: boolean;
  size?: number;
  title?: string;
};

export function Icon({
  name,
  decorative = true,
  size = 16,
  title,
  ...props
}: IconProps) {
  return (
    <svg
      {...props}
      aria-hidden={decorative}
      fill="none"
      height={size}
      role={decorative ? undefined : "img"}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.7"
      viewBox="0 0 24 24"
      width={size}
    >
      {title ? <title>{title}</title> : null}
      {renderIcon(name)}
    </svg>
  );
}

function renderIcon(name: IconName) {
  switch (name) {
    case "arrowUpRight":
      return (
        <>
          <path d="M7 17L17 7" />
          <path d="M9 7h8v8" />
        </>
      );
    case "box":
      return (
        <>
          <path d="M12 3l8 4.5v9L12 21 4 16.5v-9L12 3z" />
          <path d="M4 7.5L12 12l8-4.5" />
          <path d="M12 12v9" />
        </>
      );
    case "chevronDown":
      return <path d="M6 9l6 6 6-6" />;
    case "folder":
      return (
        <>
          <path d="M3.5 7.5h6l2 2h9v7.5A2.5 2.5 0 0118 19.5H6A2.5 2.5 0 013.5 17V7.5z" />
          <path d="M3.5 10.5h17" />
        </>
      );
    case "layout":
      return (
        <>
          <rect height="16" rx="2.5" width="18" x="3" y="4" />
          <path d="M9 4v16" />
          <path d="M9 10h12" />
        </>
      );
    case "play":
      return <path d="M8 6.5v11l9-5.5-9-5.5z" />;
    case "plus":
      return (
        <>
          <path d="M12 5v14" />
          <path d="M5 12h14" />
        </>
      );
    case "refresh":
      return (
        <>
          <path d="M20 6v5h-5" />
          <path d="M4 18v-5h5" />
          <path d="M18 11a6.5 6.5 0 00-11-3L5 11" />
          <path d="M6 13a6.5 6.5 0 0011 3l2-3" />
        </>
      );
    case "search":
      return (
        <>
          <circle cx="11" cy="11" r="6.5" />
          <path d="M16 16l4 4" />
        </>
      );
    case "server":
      return (
        <>
          <rect height="5" rx="1.5" width="16" x="4" y="4" />
          <rect height="5" rx="1.5" width="16" x="4" y="10" />
          <rect height="5" rx="1.5" width="16" x="4" y="16" />
          <path d="M8 6.5h.01" />
          <path d="M8 12.5h.01" />
          <path d="M8 18.5h.01" />
        </>
      );
    case "settings":
      return (
        <>
          <path d="M4 6h7" />
          <path d="M15 6h5" />
          <path d="M13 6a2 2 0 11-4 0 2 2 0 014 0z" />
          <path d="M4 12h3" />
          <path d="M11 12h9" />
          <path d="M11 12a2 2 0 11-4 0 2 2 0 014 0z" />
          <path d="M4 18h9" />
          <path d="M17 18h3" />
          <path d="M17 18a2 2 0 11-4 0 2 2 0 014 0z" />
        </>
      );
    case "spark":
      return (
        <>
          <path d="M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5L12 3z" />
          <path d="M18.5 15l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2z" />
        </>
      );
    case "terminal":
      return (
        <>
          <rect height="16" rx="2.5" width="18" x="3" y="4" />
          <path d="M7.5 10.5L10.5 13 7.5 15.5" />
          <path d="M12.5 15.5h4" />
        </>
      );
  }
}