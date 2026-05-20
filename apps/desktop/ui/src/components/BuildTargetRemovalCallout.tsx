import { Button } from "./Button";

type BuildTargetRemovalCalloutProps = {
  bindingImpact: string[];
  cancelDisabled?: boolean;
  confirmDisabled?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  targetName: string;
};

export function BuildTargetRemovalCallout({
  bindingImpact,
  cancelDisabled = false,
  confirmDisabled = false,
  onCancel,
  onConfirm,
  targetName,
}: BuildTargetRemovalCalloutProps) {
  return (
    <div className="wizard-callout wizard-callout--compact wizard-callout--auth">
      <div className="wizard-callout__header">
        <div>
          <p className="wizard-callout__title">Confirm build target removal</p>
          <p className="wizard-callout__copy">
            {buildRemovalImpactCopy(targetName, bindingImpact)}
          </p>
        </div>
      </div>

      <div className="wizard-callout__actions">
        <Button
          disabled={confirmDisabled}
          leadingIcon="trash"
          onClick={onConfirm}
          size="sm"
          variant="primary"
        >
          Remove build target and bindings
        </Button>
        <Button
          disabled={cancelDisabled}
          onClick={onCancel}
          size="sm"
          variant="ghost"
        >
          Cancel
        </Button>
      </div>
    </div>
  );
}

function buildRemovalImpactCopy(targetName: string, bindingImpact: string[]) {
  const resolvedTargetName = targetName.trim() || "this build target";

  if (bindingImpact.length === 0) {
    return `Removing ${resolvedTargetName} also removes its publish bindings.`;
  }

  return `Removing ${resolvedTargetName} also removes publish bindings from ${bindingImpact.join(", ")}.`;
}
