import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";

export type ConfirmDialogProps = {
  cancelLabel?: string;
  confirmLabel?: string;
  confirmVariant?: "primary" | "secondary" | "ghost";
  description?: string;
  message?: string;
  onResolve?: (value?: boolean | null) => void;
  title: string;
};

export function ConfirmDialog({
  cancelLabel = "Stay in shell",
  confirmLabel = "Confirm",
  confirmVariant = "primary",
  description,
  message,
  onResolve,
  title,
}: ConfirmDialogProps) {
  return (
    <FullScreenModal
      className="confirm-dialog__modal"
      description={description}
      onResolve={onResolve}
      title={title}
    >
      <div className="confirm-dialog">
        {message ? <p className="confirm-dialog__message">{message}</p> : null}

        <div className="confirm-dialog__actions">
          <Button
            data-overlay-autofocus
            onClick={() => onResolve?.(null)}
            size="sm"
            variant="ghost"
          >
            {cancelLabel}
          </Button>
          <Button
            onClick={() => onResolve?.(true)}
            size="sm"
            variant={confirmVariant}
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}
