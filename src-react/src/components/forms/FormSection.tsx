import React from "react";

export type FormSectionProps = {
  title?: string;
  description?: string;
  children?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
  summary?: React.ReactNode;
};

const headerStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "12px 0",
  borderBottom: "1px solid rgba(255,255,255,0.03)",
};

const FormSection: React.FC<FormSectionProps> = ({
  title,
  description,
  children,
  actions,
  className,
  summary,
}) => {
  return (
    <section className={className} style={{ marginBottom: 16 }}>
      <div style={headerStyle}>
        <div>
          {title && (
            <div style={{ fontSize: 14, fontWeight: 600 }}>{title}</div>
          )}
          {description && (
            <div style={{ fontSize: 12, opacity: 0.75 }}>{description}</div>
          )}
        </div>
        <div>{actions}</div>
      </div>
      {summary ? <div style={{ paddingTop: 12 }}>{summary}</div> : null}
      {children ? <div style={{ paddingTop: 12 }}>{children}</div> : null}
    </section>
  );
};

export default FormSection;
