import { useState } from "react";

import { Button } from "./Button";
import { TextField } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { SurfacePanel } from "./Surface";
import { useLocalization } from "../LocalizationProvider";

export type FullScreenFileBrowserProps = {
  initialPath?: string;
  onResolve?: (value?: string | null) => void;
};

const FullScreenFileBrowser = ({
  initialPath,
  onResolve,
}: FullScreenFileBrowserProps) => {
  const { t } = useLocalization();
  const [path, setPath] = useState(initialPath ?? "");

  const normalizedPath = path.trim();

  return (
    <FullScreenModal
      description={t(
        "full_screen_file_browser.description",
        "The native host picker is unavailable. Paste or type an absolute path to continue.",
      )}
      onResolve={onResolve}
      title={t("full_screen_file_browser.title", "Enter path manually")}
    >
      <div className="select-list-modal">
        <TextField
          autoComplete="off"
          data-overlay-autofocus
          hint={t(
            "full_screen_file_browser.field.hint",
            "Absolute host path",
          )}
          label={t("full_screen_file_browser.field.label", "Path")}
          onChange={(event) => setPath(event.target.value)}
          placeholder={t(
            "full_screen_file_browser.field.placeholder",
            "C:/Projects/MyUnityProject",
          )}
          value={path}
        />

        <SurfacePanel
          description={t(
            "full_screen_file_browser.panel.description",
            "This fallback keeps the workflow unblocked when the desktop picker cannot be opened.",
          )}
          summary={
            <p className="full-screen-file-browser__summary">
              {t(
                "full_screen_file_browser.panel.summary",
                "Provide the full host path exactly as it should be persisted in the repository settings.",
              )}
            </p>
          }
          title={t(
            "full_screen_file_browser.panel.title",
            "Manual path entry",
          )}
          tone="inset"
        >
          <div className="input-with-picker__actions">
            <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
              {t("full_screen_file_browser.actions.cancel", "Cancel")}
            </Button>
            <Button
              disabled={!normalizedPath}
              onClick={() => onResolve?.(normalizedPath)}
              size="sm"
              variant="primary"
            >
              {t("full_screen_file_browser.actions.use_path", "Use path")}
            </Button>
          </div>
        </SurfacePanel>
      </div>
    </FullScreenModal>
  );
};

export default FullScreenFileBrowser;
