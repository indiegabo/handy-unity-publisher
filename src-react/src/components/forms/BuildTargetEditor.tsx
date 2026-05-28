import React from "react";

export type BuildTargetEditorProps = {
  target: any;
  onRemove?: () => void;
};

const containerStyle: React.CSSProperties = {
  border: "1px solid rgba(255,255,255,0.03)",
  padding: 12,
  borderRadius: 6,
  marginBottom: 8,
};

const BuildTargetEditor: React.FC<BuildTargetEditorProps> = ({
  target,
  onRemove,
}) => {
  return (
    <div style={containerStyle}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <div style={{ fontWeight: 600 }}>
          {target?.name ?? "Untitled target"}
        </div>
        <div>
          <button
            onClick={onRemove}
            style={{
              background: "transparent",
              border: "none",
              color: "#FF6B6B",
            }}
          >
            Remove
          </button>
        </div>
      </div>
      <div style={{ marginTop: 8, fontSize: 13, opacity: 0.85 }}>
        {target?.targetPlatform ?? "Platform not set"}
      </div>
    </div>
  );
};

export default BuildTargetEditor;
