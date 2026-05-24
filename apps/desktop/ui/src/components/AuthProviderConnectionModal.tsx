import { useState } from "react";

import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { Badge, MetaItem, MetaRow, SummaryStrip } from "./Surface";
import {
  buildAuthProviderConnectionResult,
  buildAuthProviderLifecycleSnapshot,
  buildAuthProviderSummaryRows,
  formatAuthProviderStatus,
  formatBoundRepositoryCount,
  resolveAuthProviderTone,
  type AuthProviderConnectionResult,
} from "./authProviderPresentation";
import StepFlow, { type StepFlowStep } from "./wizard/StepFlow";
import { useLocalization } from "../LocalizationProvider";
import { loginWithGithubAuth, type AuthProviderStatus } from "../services/auth";

type AuthProviderConnectionModalProps = {
  onResolve?: (value?: AuthProviderConnectionResult | null) => void;
  provider: AuthProviderStatus;
};

type AuthProviderConnectionStepKey = "summary" | "browser";

export function AuthProviderConnectionModal({
  onResolve,
  provider,
}: AuthProviderConnectionModalProps) {
  const { t } = useLocalization();
  const [currentStepKey, setCurrentStepKey] =
    useState<AuthProviderConnectionStepKey>("summary");
  const [actionError, setActionError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const lifecycleSnapshot = buildAuthProviderLifecycleSnapshot(t, provider);
  const connectionSteps = buildAuthProviderConnectionSteps(t);

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
      onResolve?.(buildAuthProviderConnectionResult(t, provider, nextProvider));
    } catch (error) {
      setActionError(buildAuthProviderConnectionErrorMessage(t, error));
    } finally {
      setIsSubmitting(false);
    }
  };

  const currentStepSummary =
    currentStepKey === "summary" ? (
      <MetaRow>
        <MetaItem
          label={t("auth_provider_connection.summary.provider", "Provider")}
        >
          {provider.label}
        </MetaItem>
        <MetaItem label={t("auth_provider_connection.summary.usage", "Usage")}>
          {formatBoundRepositoryCount(t, provider.bound_repository_count)}
        </MetaItem>
      </MetaRow>
    ) : (
      <MetaRow>
        <MetaItem
          label={t(
            "auth_provider_connection.summary.current_state",
            "Current state",
          )}
        >
          {formatAuthProviderStatus(t, provider.status)}
        </MetaItem>
        <MetaItem
          label={t("auth_provider_connection.summary.action", "Action")}
        >
          {provider.status === "connected"
            ? t("auth_provider_connection.action.reconnect", "Reconnect")
            : t("auth_provider_connection.action.bind", "Bind")}
        </MetaItem>
      </MetaRow>
    );

  return (
    <FullScreenModal
      description={t(
        "auth_provider_connection.modal.description",
        "Review the selected provider and open the browser login flow only when host-backed credentials need to be created or refreshed.",
      )}
      dismissible={!isSubmitting}
      onResolve={onResolve}
      title={t(
        "auth_provider_connection.modal.title",
        "{{providerLabel}} connection",
        { providerLabel: provider.label },
      )}
    >
      {actionError ? (
        <p className="feed-banner feed-banner--error">{actionError}</p>
      ) : null}

      <StepFlow
        activeStepKey={currentStepKey}
        endActions={
          currentStepKey === "summary" ? (
            <Button
              data-overlay-autofocus
              onClick={handleAdvanceStep}
              size="sm"
              variant="primary"
            >
              {t("auth_provider_connection.actions.continue", "Continue")}
            </Button>
          ) : (
            <Button
              data-overlay-autofocus
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
                t,
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
        progressDescription={t(
          "auth_provider_connection.progress.description",
          "The provider stays unchanged until the browser login completes successfully. Closing this overlay leaves the current shell state untouched.",
        )}
        progressEyebrow={t(
          "auth_provider_connection.progress.eyebrow",
          "Auth Flow",
        )}
        progressSummary={
          <MetaRow>
            <MetaItem
              label={t("auth_provider_connection.summary.provider", "Provider")}
            >
              {provider.label}
            </MetaItem>
            <MetaItem
              label={t(
                "auth_provider_connection.progress.selected_credential",
                "Selected credential",
              )}
            >
              {provider.credential_name ||
                t(
                  "auth_providers.presentation.no_reusable_credential",
                  "No reusable credential",
                )}
            </MetaItem>
          </MetaRow>
        }
        progressTitle={t(
          "auth_provider_connection.progress.title",
          "Connection Stages",
        )}
        startActions={
          currentStepKey === "browser" ? (
            <Button
              disabled={isSubmitting}
              onClick={handleRetreatStep}
              size="sm"
              variant="ghost"
            >
              {t("auth_provider_connection.actions.back", "Back")}
            </Button>
          ) : null
        }
        stepSummary={currentStepSummary}
        steps={connectionSteps}
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
              {formatAuthProviderStatus(t, provider.status)}
            </Badge>
          </header>

          <p className="auth-provider-card__copy">{provider.status_message}</p>

          <SummaryStrip className="auth-provider-card__summary-strip">
            {buildAuthProviderSummaryRows(t, provider, lifecycleSnapshot, {
              includeLifecycleRow: false,
            }).map((summaryRow, summaryRowIndex) => (
              <MetaRow
                className="auth-provider-card__summary"
                key={`${provider.provider_id}-summary-${summaryRowIndex}`}
              >
                {summaryRow.map((item) => (
                  <MetaItem key={item.label} label={item.label}>
                    {item.value}
                  </MetaItem>
                ))}
              </MetaRow>
            ))}
          </SummaryStrip>

          <p className="auth-provider-card__copy">
            {currentStepKey === "summary"
              ? t(
                  "auth_provider_connection.copy.summary",
                  "Proceed to the browser stage only when this provider should be rebound or refreshed through Git Credential Manager.",
                )
              : provider.status === "connected"
                ? t(
                    "auth_provider_connection.copy.browser.connected",
                    "If the host reports that the current credential expired or was rejected, run the browser flow again to repair the binding.",
                  )
                : t(
                    "auth_provider_connection.copy.browser.disconnected",
                    "Open the browser flow to create the host-backed credential that repository access will reuse.",
                  )}
          </p>

          {currentStepKey === "browser" ? (
            <p className="auth-provider-card__copy">
              {actionError
                ? t(
                    "auth_provider_connection.copy.error",
                    "Review the error, then retry the browser flow or close this overlay to leave the current provider unchanged.",
                  )
                : t(
                    "auth_provider_connection.copy.success",
                    "The overlay will close only after the browser flow returns an updated provider state.",
                  )}
            </p>
          ) : null}
        </section>
      </StepFlow>
    </FullScreenModal>
  );
}

function buildAuthProviderConnectionSteps(
  t: ReturnType<typeof useLocalization>["t"],
): readonly StepFlowStep<AuthProviderConnectionStepKey>[] {
  return [
    {
      description: t(
        "auth_provider_connection.steps.summary.description",
        "Review the selected provider state before starting the browser-driven credential flow.",
      ),
      key: "summary",
      label: t("auth_provider_connection.steps.summary.label", "Summary"),
    },
    {
      description: t(
        "auth_provider_connection.steps.browser.description",
        "Run the browser flow through Git Credential Manager and recover here if the host reports a transient auth failure.",
      ),
      key: "browser",
      label: t("auth_provider_connection.steps.browser.label", "Browser"),
    },
  ];
}

function buildBrowserLoginLabel(
  t: ReturnType<typeof useLocalization>["t"],
  status: string,
  hasActionError: boolean,
  isSubmitting: boolean,
) {
  if (isSubmitting) {
    return t("auth_provider_connection.actions.connecting", "Connecting...");
  }

  if (hasActionError) {
    return status === "connected"
      ? t(
          "auth_provider_connection.actions.retry_reconnect",
          "Retry reconnect with browser",
        )
      : t(
          "auth_provider_connection.actions.retry_login",
          "Retry browser login",
        );
  }

  return status === "connected"
    ? t("auth_provider_connection.actions.reconnect", "Reconnect with browser")
    : t("auth_provider_connection.actions.login", "Log in with browser");
}

function buildAuthProviderConnectionErrorMessage(
  t: ReturnType<typeof useLocalization>["t"],
  error: unknown,
) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t(
    "auth_provider_connection.error.fallback",
    "The desktop shell could not complete the browser login flow.",
  );
}

export default AuthProviderConnectionModal;
