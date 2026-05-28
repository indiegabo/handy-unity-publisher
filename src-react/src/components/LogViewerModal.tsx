import { useState } from "react";

import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { SummaryStrip } from "./Surface";

export type LogViewerModalProps = {
  content: string;
  description?: string;
  downloadFileName?: string;
  initialWrap?: boolean;
  meta?: string;
  onResolve?: (value?: null) => void;
  title: string;
};

export function LogViewerModal({
  content,
  description,
  downloadFileName,
  initialWrap = true,
  meta,
  onResolve,
  title,
}: LogViewerModalProps) {
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [isWrapped, setIsWrapped] = useState(initialWrap);

  const handleCopy = async () => {
    if (!navigator.clipboard?.writeText) {
      setActionMessage("Clipboard access is unavailable in this shell session.");
      return;
    }

    try {
      await navigator.clipboard.writeText(content);
      setActionMessage("Copied the current viewer content to the clipboard.");
    } catch {
      setActionMessage("The shell could not copy the current viewer content.");
    }
  };

  const handleDownload = () => {
    if (
      typeof Blob === "undefined" ||
      typeof URL === "undefined" ||
      typeof URL.createObjectURL !== "function" ||
      typeof URL.revokeObjectURL !== "function"
    ) {
      setActionMessage("File download is unavailable in this shell session.");
      return;
    }

    const resolvedFileName = resolveLogViewerDownloadFileName(
      downloadFileName,
      title,
    );
    let objectUrl: string | null = null;

    try {
      objectUrl = URL.createObjectURL(
        new Blob([content], {
          type: "text/plain;charset=utf-8",
        }),
      );

      const link = document.createElement("a");
      link.href = objectUrl;
      link.download = resolvedFileName;
      link.rel = "noopener";
      link.style.display = "none";

      document.body.append(link);
      try {
        link.click();
      } finally {
        link.remove();
      }

      setActionMessage(`Downloaded ${resolvedFileName}.`);
    } catch {
      setActionMessage("The shell could not download the current viewer content.");
    } finally {
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
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
          <Button onClick={handleDownload} size="sm" variant="ghost">
            Download content
          </Button>
        </div>

        {meta || actionMessage ? (
          <SummaryStrip className="log-viewer-modal__summary-strip">
            {meta ? <p className="log-viewer-modal__meta">{meta}</p> : null}
            {actionMessage ? (
              <p className="log-viewer-modal__meta">{actionMessage}</p>
            ) : null}
          </SummaryStrip>
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

function resolveLogViewerDownloadFileName(
  downloadFileName: string | undefined,
  title: string,
) {
  const rawValue =
    downloadFileName?.trim() || title.trim() || "log-viewer-content.txt";
  const sanitizedValue = rawValue
    .replace(/[<>:"/\\|?*\u0000-\u001F]+/g, "-")
    .replace(/\s+/g, " ")
    .trim();
  const normalizedValue = sanitizedValue || "log-viewer-content";

  return /\.[a-z0-9]+$/i.test(normalizedValue)
    ? normalizedValue
    : `${normalizedValue}.txt`;
}
