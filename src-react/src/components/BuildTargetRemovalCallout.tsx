import { Button } from "./Button";
import { useLocalization } from "../LocalizationProvider";

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
  const { t } = useLocalization();

  return (
    <div className="wizard-callout wizard-callout--compact wizard-callout--auth">
      <div className="wizard-callout__header">
        <div>
          <p className="wizard-callout__title">
            {t(
              "project_shared.build_target_removal.title",
              "Confirm build target removal",
            )}
          </p>
          <p className="wizard-callout__copy">
            {buildRemovalImpactCopy(t, targetName, bindingImpact)}
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
          {t(
            "project_shared.build_target_removal.actions.confirm",
            "Remove build target and bindings",
          )}
        </Button>
        <Button
          disabled={cancelDisabled}
          onClick={onCancel}
          size="sm"
          variant="ghost"
        >
          {t("project_shared.build_target_removal.actions.cancel", "Cancel")}
        </Button>
      </div>
    </div>
  );
}

function buildRemovalImpactCopy(
  t: ReturnType<typeof useLocalization>["t"],
  targetName: string,
  bindingImpact: string[],
) {
  const resolvedTargetName =
    targetName.trim() ||
    t(
      "project_shared.build_target_removal.target_fallback",
      "this build target",
    );

  if (bindingImpact.length === 0) {
    return t(
      "project_shared.build_target_removal.copy.no_bindings",
      "Removing {{targetName}} also removes its publish bindings.",
      { targetName: resolvedTargetName },
    );
  }

  return t(
    "project_shared.build_target_removal.copy.with_bindings",
    "Removing {{targetName}} also removes publish bindings from {{bindingImpact}}.",
    {
      bindingImpact: bindingImpact.join(", "),
      targetName: resolvedTargetName,
    },
  );
}
