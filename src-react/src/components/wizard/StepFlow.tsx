import type { ReactNode } from "react";

import { SurfacePanel } from "../Surface";
import { useLocalization } from "../../LocalizationProvider";

export type StepFlowStep<TStepKey extends string = string> = {
  description: string;
  key: TStepKey;
  label: string;
};

type StepFlowProps<TStepKey extends string = string> = {
  activeStepKey: TStepKey;
  children: ReactNode;
  endActions?: ReactNode;
  isStepSelectable?: (step: StepFlowStep<TStepKey>, index: number) => boolean;
  onStepSelect?: (stepKey: TStepKey) => void;
  progressDescription?: string;
  progressEyebrow?: string;
  progressSummary?: ReactNode;
  progressTitle?: string;
  startActions?: ReactNode;
  stepSummary?: ReactNode;
  steps: readonly StepFlowStep<TStepKey>[];
};

export function StepFlow<TStepKey extends string = string>({
  activeStepKey,
  children,
  endActions,
  isStepSelectable,
  onStepSelect,
  progressDescription,
  progressEyebrow,
  progressSummary,
  progressTitle,
  startActions,
  stepSummary,
  steps,
}: StepFlowProps<TStepKey>) {
  const { t } = useLocalization();
  const activeStepIndex = Math.max(
    0,
    steps.findIndex((step) => step.key === activeStepKey),
  );
  const activeStep = steps[activeStepIndex] ?? steps[0];
  const resolvedProgressDescription =
    progressDescription ??
    t(
      "wizard.step_flow.progress_description",
      "Move across completed steps without losing the current draft.",
    );
  const resolvedProgressEyebrow =
    progressEyebrow ?? t("wizard.step_flow.progress_eyebrow", "Progress");
  const resolvedProgressTitle =
    progressTitle ?? t("wizard.step_flow.progress_title", "Steps");
  const progressAriaLabel = t(
    "wizard.step_flow.progress_aria_label",
    "Step flow progress",
  );
  const currentStepEyebrow = t(
    "wizard.step_flow.current_step_eyebrow",
    "Step {{current}} of {{total}}",
    {
      current: activeStepIndex + 1,
      total: steps.length,
    },
  );

  return (
    <>
      <div className="wizard-stage-shell">
        <SurfacePanel
          className="wizard-progress-panel"
          description={resolvedProgressDescription}
          eyebrow={resolvedProgressEyebrow}
          headerSeparated
          summary={progressSummary}
          title={resolvedProgressTitle}
          tone="inset"
        >
          <div className="wizard-stepper" aria-label={progressAriaLabel}>
            {steps.map((step, index) => {
              const selectable = isStepSelectable
                ? isStepSelectable(step, index)
                : index <= activeStepIndex;

              return (
                <button
                  aria-current={index === activeStepIndex ? "step" : undefined}
                  className={joinClassNames(
                    "wizard-stepper__item",
                    index === activeStepIndex &&
                      "wizard-stepper__item--current",
                    index < activeStepIndex && "wizard-stepper__item--complete",
                  )}
                  disabled={!selectable}
                  key={step.key}
                  onClick={() => onStepSelect?.(step.key)}
                  type="button"
                >
                  <span className="wizard-stepper__index">{index + 1}</span>
                  <span className="wizard-stepper__label">{step.label}</span>
                </button>
              );
            })}
          </div>
        </SurfacePanel>

        <div className="wizard-stage-content-shell">
          <SurfacePanel
            className="wizard-stage-panel"
            description={activeStep.description}
            eyebrow={currentStepEyebrow}
            headerSeparated
            summary={stepSummary}
            title={activeStep.label}
            tone="section"
          >
            {children}
          </SurfacePanel>
        </div>
      </div>

      <footer className="wizard-footer">
        <div className="wizard-footer__slot wizard-footer__slot--start">
          {startActions}
        </div>

        <div className="wizard-footer__slot wizard-footer__slot--end">
          {endActions}
        </div>
      </footer>
    </>
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}

export default StepFlow;
