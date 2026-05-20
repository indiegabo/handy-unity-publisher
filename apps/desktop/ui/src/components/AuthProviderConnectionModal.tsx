import { useState } from "react";

import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { Badge, MetaItem, MetaRow } from "./Surface";
import {
  buildAuthProviderConnectionResult,
  buildAuthProviderLifecycleSnapshot,
  formatAuthProviderStatus,
  formatBoundRepositoryCount,
  resolveAuthProviderTone,
  type AuthProviderConnectionResult,
} from "./authProviderPresentation";
import StepFlow, { type StepFlowStep } from "./wizard/StepFlow";
import { loginWithGithubAuth, type AuthProviderStatus } from "../services/auth";

type AuthProviderConnectionModalProps = {
  onResolve?: (value?: AuthProviderConnectionResult | null) => void;
  provider: AuthProviderStatus;
};

type AuthProviderConnectionStepKey = "summary" | "browser";

const AUTH_PROVIDER_CONNECTION_STEPS: readonly StepFlowStep<AuthProviderConnectionStepKey>[] =
  [
    {
      description:
        "Review the selected provider state before starting the browser-driven credential flow.",
      key: "summary",
      label: "Summary",
    },
    {
      description:
        "Run the browser flow through Git Credential Manager and recover here if the host reports a transient auth failure.",
      key: "browser",
      label: "Browser",
    },
  ];

export function AuthProviderConnectionModal({
  onResolve,
  provider,
}: AuthProviderConnectionModalProps) {
  const [currentStepKey, setCurrentStepKey] =
    useState<AuthProviderConnectionStepKey>("summary");
  const [actionError, setActionError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const lifecycleSnapshot = buildAuthProviderLifecycleSnapshot(provider);

  const handleAdvanceStep = () => {
    setActionError(null);
    setCurrentStepKey("browser");
  };

  const handleRetreatStep = () => {
    if (isSubmitting) {
      return;
    }

    setActionError(null);
    setCurrentStepKey("summary");
  };

  const handleBrowserLogin = async () => {
    setIsSubmitting(true);
    setActionError(null);

    try {
      const nextProvider = await loginWithGithubAuth({
        force: provider.status === "connected",
      });
      onResolve?.(buildAuthProviderConnectionResult(provider, nextProvider));
    } catch (error) {
      setActionError(buildAuthProviderConnectionErrorMessage(error));
    } finally {
      setIsSubmitting(false);
    }
  };

  const currentStepSummary =
    currentStepKey === "summary" ? (
      <MetaRow>
        <MetaItem label="Provider">{provider.label}</MetaItem>
        <MetaItem label="Usage">
          {formatBoundRepositoryCount(provider.bound_repository_count)}
        </MetaItem>
      </MetaRow>
    ) : (
      <MetaRow>
        <MetaItem label="Current state">
          {formatAuthProviderStatus(provider.status)}
        </MetaItem>
        <MetaItem label="Action">
          {provider.status === "connected" ? "Reconnect" : "Bind"}
        </MetaItem>
      </MetaRow>
    );

  return (
    <FullScreenModal
      description="Review the selected provider and open the browser login flow only when host-backed credentials need to be created or refreshed."
      dismissible={!isSubmitting}
      onResolve={onResolve}
      title={`${provider.label} connection`}
    >
      {actionError ? (
        <p className="feed-banner feed-banner--error">{actionError}</p>
      ) : null}

      <StepFlow
        activeStepKey={currentStepKey}
        endActions={
          currentStepKey === "summary" ? (
            <Button onClick={handleAdvanceStep} size="sm" variant="primary">
              Continue
            </Button>
          ) : (
            <Button
              disabled={isSubmitting}
              leadingIcon="arrowUpRight"
              onClick={() => {
                void handleBrowserLogin();
              }}
              size="sm"
              variant={
                provider.status === "connected" ? "secondary" : "primary"
              }
            >
              {buildBrowserLoginLabel(
                provider.status,
                actionError !== null,
                isSubmitting,
              )}
            </Button>
          )
        }
        isStepSelectable={(step) =>
          step.key === "summary" || currentStepKey === "browser"
        }
        onStepSelect={(stepKey) => {
          if (stepKey === "summary") {
            handleRetreatStep();
          }
        }}
        progressDescription="The provider stays unchanged until the browser login completes successfully. Closing this overlay leaves the current shell state untouched."
        progressEyebrow="Auth Flow"
        progressSummary={
          <MetaRow>
            <MetaItem label="Provider">{provider.label}</MetaItem>
            <MetaItem label="Selected credential">
              {provider.credential_name || "No reusable credential"}
            </MetaItem>
          </MetaRow>
        }
        progressTitle="Connection Stages"
        startActions={
          currentStepKey === "browser" ? (
            <Button
              disabled={isSubmitting}
              onClick={handleRetreatStep}
              size="sm"
              variant="ghost"
            >
              Back
            </Button>
          ) : null
        }
        stepSummary={currentStepSummary}
        steps={AUTH_PROVIDER_CONNECTION_STEPS}
      >
        <section className="auth-provider-card">
          <header className="auth-provider-card__header">
            <div className="auth-provider-card__title-block">
              <h3 className="auth-provider-card__title">{provider.label}</h3>
              <p className="auth-provider-card__copy">
                {provider.instance_url}
              </p>
            </div>
            <Badge tone={resolveAuthProviderTone(provider.status)}>
              {formatAuthProviderStatus(provider.status)}
            </Badge>
          </header>

          <p className="auth-provider-card__copy">{provider.status_message}</p>

          <MetaRow className="auth-provider-card__summary">
            <MetaItem label="Credential">
              {provider.credential_name || "No reusable credential"}
            </MetaItem>
            <MetaItem label="Usage">
              {formatBoundRepositoryCount(provider.bound_repository_count)}
            </MetaItem>
          </MetaRow>

          <MetaRow className="auth-provider-card__summary">
            <MetaItem label="Stored">
              {lifecycleSnapshot.storedAtLabel}
            </MetaItem>
            <MetaItem label="Refreshed">
              {lifecycleSnapshot.refreshedAtLabel}
            </MetaItem>
          </MetaRow>

          <p className="auth-provider-card__copy">
            {currentStepKey === "summary"
              ? "Proceed to the browser stage only when this provider should be rebound or refreshed through Git Credential Manager."
              : provider.status === "connected"
                ? "If the host reports that the current credential expired or was rejected, run the browser flow again to repair the binding."
                : "Open the browser flow to create the host-backed credential that repository access will reuse."}
          </p>

          {currentStepKey === "browser" ? (
            <p className="auth-provider-card__copy">
              {actionError
                ? "Review the error, then retry the browser flow or close this overlay to leave the current provider unchanged."
                : "The overlay will close only after the browser flow returns an updated provider state."}
            </p>
          ) : null}
        </section>
      </StepFlow>
    </FullScreenModal>
  );
}

function buildBrowserLoginLabel(
  status: string,
  hasActionError: boolean,
  isSubmitting: boolean,
) {
  if (isSubmitting) {
    return "Connecting...";
  }

  if (hasActionError) {
    return status === "connected"
      ? "Retry reconnect with browser"
      : "Retry browser login";
  }

  return status === "connected"
    ? "Reconnect with browser"
    : "Log in with browser";
}

function buildAuthProviderConnectionErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not complete the browser login flow.";
}

export default AuthProviderConnectionModal;
