import { useState } from "react";

import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";

export type LogViewerModalProps = {
  content: string;
  description?: string;
  initialWrap?: boolean;
  meta?: string;
  onResolve?: (value?: null) => void;
  title: string;
};

export function LogViewerModal({
  content,
  description,
  initialWrap = true,
  meta,
  onResolve,
  title,
}: LogViewerModalProps) {
  const [copyMessage, setCopyMessage] = useState<string | null>(null);
  const [isWrapped, setIsWrapped] = useState(initialWrap);

  const handleCopy = async () => {
    if (!navigator.clipboard?.writeText) {
      setCopyMessage("Clipboard access is unavailable in this shell session.");
      return;
    }

    try {
      await navigator.clipboard.writeText(content);
      setCopyMessage("Copied the current viewer content to the clipboard.");
    } catch {
      setCopyMessage("The shell could not copy the current viewer content.");
    }
  };

  return (
    <FullScreenModal
      className="log-viewer-modal__modal"
      description={description}
      onResolve={onResolve}
      title={title}
    >
      <div className="log-viewer-modal">
        <div className="log-viewer-modal__toolbar">
          <Button
            data-overlay-autofocus
            onClick={() => setIsWrapped((current) => !current)}
            size="sm"
            variant="ghost"
          >
            {isWrapped ? "Preserve lines" : "Wrap lines"}
          </Button>
          <Button onClick={() => void handleCopy()} size="sm" variant="ghost">
            Copy content
          </Button>
        </div>

        {meta ? <p className="log-viewer-modal__meta">{meta}</p> : null}
        {copyMessage ? (
          <p className="log-viewer-modal__meta">{copyMessage}</p>
        ) : null}

        <pre
          className={joinClassNames(
            "log-viewer-modal__content",
            isWrapped && "log-viewer-modal__content--wrapped",
          )}
        >
          {content}
        </pre>
      </div>
    </FullScreenModal>
  );
}

function joinClassNames(...tokens: Array<string | false>) {
  return tokens.filter(Boolean).join(" ");
}
