import { useState } from "react";

import { Button } from "./Button";
import { TextField } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { SurfacePanel } from "./Surface";

export type FullScreenFileBrowserProps = {
  initialPath?: string;
  onResolve?: (value?: string | null) => void;
};

const FullScreenFileBrowser = ({
  initialPath,
  onResolve,
}: FullScreenFileBrowserProps) => {
  const [path, setPath] = useState(initialPath ?? "");

  const normalizedPath = path.trim();

  return (
    <FullScreenModal
      description="The native host picker is unavailable. Paste or type an absolute path to continue."
      onResolve={onResolve}
      title="Enter path manually"
    >
      <div className="select-list-modal">
        <TextField
          autoComplete="off"
          hint="Absolute host path"
          label="Path"
          onChange={(event) => setPath(event.target.value)}
          placeholder="C:/Projects/MyUnityProject"
          value={path}
        />

        <SurfacePanel
          description="This fallback keeps the workflow unblocked when the desktop picker cannot be opened."
          title="Manual path entry"
          tone="inset"
        >
          <p className="project-list-card__summary">
            Provide the full host path exactly as it should be persisted in the
            repository settings.
          </p>

          <div className="input-with-picker__actions">
            <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
              Cancel
            </Button>
            <Button
              disabled={!normalizedPath}
              onClick={() => onResolve?.(normalizedPath)}
              size="sm"
              variant="primary"
            >
              Use path
            </Button>
          </div>
        </SurfacePanel>
      </div>
    </FullScreenModal>
  );
};

export default FullScreenFileBrowser;
