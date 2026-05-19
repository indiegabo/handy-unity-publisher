import React from "react";

export type FormSectionProps = {
  title?: string;
  description?: string;
  children?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
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
      <div style={{ paddingTop: 12 }}>{children}</div>
    </section>
  );
};

export default FormSection;
