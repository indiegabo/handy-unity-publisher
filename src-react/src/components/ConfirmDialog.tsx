import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { useLocalization } from "../LocalizationProvider";

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
  cancelLabel,
  confirmLabel,
  confirmVariant = "primary",
  description,
  message,
  onResolve,
  title,
}: ConfirmDialogProps) {
  const { t } = useLocalization();
  const resolvedCancelLabel =
    cancelLabel ?? t("confirm_dialog.actions.cancel", "Stay in shell");
  const resolvedConfirmLabel =
    confirmLabel ?? t("confirm_dialog.actions.confirm", "Confirm");

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
            {resolvedCancelLabel}
          </Button>
          <Button
            onClick={() => onResolve?.(true)}
            size="sm"
            variant={confirmVariant}
          >
            {resolvedConfirmLabel}
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}
