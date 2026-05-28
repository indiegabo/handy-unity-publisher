import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { SurfacePanel } from "./Surface";
import { useLocalization, type Translate } from "../LocalizationProvider";
import type {
  SaveSecretCredentialInput,
  SecretCredentialKind,
} from "../services/projects";

type RepositoryCredentialComposerProps = {
  initialCredential?: RepositoryCredentialInitialValue | null;
  isSaving: boolean;
  onCancel: () => void;
  onSave: (input: SaveSecretCredentialInput) => Promise<void> | void;
  providerLabel: string;
  renderSurface?: boolean;
  saveError: string | null;
  scope?: "repository" | "publish";
};

type RepositoryCredentialDraftKind = Exclude<
  SecretCredentialKind,
  "git-http-github-host-login"
>;

export type RepositoryCredentialInitialValue = {
  credentialId: number | null;
  kind: RepositoryCredentialDraftKind;
  name: string;
};

type RepositoryCredentialDraft = {
  credentialId: number | null;
  kind: RepositoryCredentialDraftKind;
  apiKey: string;
  name: string;
  password: string;
  token: string;
  username: string;
};

type RepositoryCredentialDraftErrors = {
  apiKey?: string;
  name?: string;
  password?: string;
  token?: string;
  username?: string;
};

export function RepositoryCredentialComposer({
  initialCredential = null,
  isSaving,
  onCancel,
  onSave,
  providerLabel,
  renderSurface = true,
  saveError,
  scope = "repository",
}: RepositoryCredentialComposerProps) {
  const { t } = useLocalization();
  const isEditing = initialCredential?.credentialId != null;
  const [draft, setDraft] = useState<RepositoryCredentialDraft>(() => ({
    credentialId: initialCredential?.credentialId ?? null,
    kind:
      initialCredential?.kind ??
      (scope === "publish" ? "itch-api-key" : "git-http-basic"),
    apiKey: "",
    name: initialCredential?.name ?? "",
    password: "",
    token: "",
    username: "",
  }));
  const [errors, setErrors] = useState<RepositoryCredentialDraftErrors>({});
  const credentialKindOptions =
    scope === "publish"
      ? buildPublishCredentialKindOptions(t)
      : buildRepositoryCredentialKindOptions(t);
  const panelTitle =
    scope === "publish"
      ? isEditing
        ? t(
            "credential_composer.publish.edit.title",
            "Edit publish credential",
          )
        : t(
            "credential_composer.publish.create.title",
            "New publish credential",
          )
      : isEditing
        ? t(
            "credential_composer.repository.edit.title",
            "Edit repository credential",
          )
        : t(
            "credential_composer.repository.create.title",
            "New repository credential",
          );
  const panelDescription =
    scope === "publish"
      ? isEditing
        ? t(
            "credential_composer.publish.edit.description",
            "Update one reusable {{providerLabel}} credential and replace the stored secret used by publish destinations.",
            { providerLabel },
          )
        : t(
            "credential_composer.publish.create.description",
            "Create one reusable {{providerLabel}} credential and bind it to this publish destination.",
            { providerLabel },
          )
      : isEditing
        ? t(
            "credential_composer.repository.edit.description",
            "Update one reusable {{providerLabel}} credential and replace the stored secret used by repository flows.",
            { providerLabel },
          )
        : t(
            "credential_composer.repository.create.description",
            "Create one reusable {{providerLabel}} credential and connect it to this project.",
            { providerLabel },
          );
  const credentialNamePlaceholder =
    scope === "publish"
      ? t(
          "credential_composer.publish.name_placeholder",
          "{{providerLabel}} publish credential",
          { providerLabel },
        )
      : t(
          "credential_composer.repository.name_placeholder",
          "{{providerLabel}} repository credential",
          { providerLabel },
        );

  const content = (
    <>
      <TextField
        autoComplete="off"
        data-overlay-autofocus
        error={errors.name}
        hint={t(
          "credential_composer.name.hint",
          "Use a unique reusable credential name.",
        )}
        label={t("credential_composer.name.label", "Credential name")}
        onChange={(event) =>
          handleDraftFieldChange("name", event.currentTarget.value)
        }
        placeholder={credentialNamePlaceholder}
        value={draft.name}
      />

      <SelectField
        label={t("credential_composer.kind.label", "Credential type")}
        onChange={(event) =>
          handleDraftFieldChange(
            "kind",
            event.currentTarget.value as RepositoryCredentialDraftKind,
          )
        }
        options={credentialKindOptions}
        value={draft.kind}
      />

      {isEditing ? (
        <p className="wizard-callout__copy">
          {t(
            "credential_composer.edit.secret_hint",
            "Re-enter the secret material to replace the stored credential value.",
          )}
        </p>
      ) : null}

      {draft.kind === "git-http-basic" ? (
        <>
          <TextField
            autoComplete="username"
            error={errors.username}
            hint={t(
              "credential_composer.username.hint",
              "The Git host username or service account login.",
            )}
            label={t("credential_composer.username.label", "Username")}
            onChange={(event) =>
              handleDraftFieldChange("username", event.currentTarget.value)
            }
            value={draft.username}
          />
          <TextField
            autoComplete="new-password"
            error={errors.password}
            hint={t(
              "credential_composer.password.hint",
              "Stored exactly as provided for HTTP basic authentication.",
            )}
            label={t(
              "credential_composer.password.label",
              "Password or token",
            )}
            onChange={(event) =>
              handleDraftFieldChange("password", event.currentTarget.value)
            }
            type="password"
            value={draft.password}
          />
        </>
      ) : draft.kind === "git-http-bearer" ? (
        <TextField
          autoComplete="new-password"
          error={errors.token}
          hint={t(
            "credential_composer.token.hint",
            "Stored exactly as provided for bearer authorization.",
          )}
          label={t("credential_composer.token.label", "Bearer token")}
          onChange={(event) =>
            handleDraftFieldChange("token", event.currentTarget.value)
          }
          type="password"
          value={draft.token}
        />
      ) : (
        <TextField
          autoComplete="new-password"
          error={errors.apiKey}
          hint={t(
            "credential_composer.api_key.hint",
            "Stored exactly as provided for the Itch butler API key.",
          )}
          label={t("credential_composer.api_key.label", "API key")}
          onChange={(event) =>
            handleDraftFieldChange("apiKey", event.currentTarget.value)
          }
          type="password"
          value={draft.apiKey}
        />
      )}

      {saveError ? <p className="ui-field__error">{saveError}</p> : null}

      <div className="wizard-callout__actions">
        <Button
          disabled={isSaving}
          leadingIcon="plus"
          onClick={() => void handleSave()}
          size="sm"
          variant="primary"
        >
          {isSaving
            ? isEditing
              ? t(
                  "credential_composer.actions.saving_changes",
                  "Saving changes...",
                )
              : t(
                  "credential_composer.actions.saving_credential",
                  "Saving credential...",
                )
            : isEditing
              ? t(
                  "credential_composer.actions.save_changes",
                  "Save changes",
                )
              : t(
                  "credential_composer.actions.save_credential",
                  "Save credential",
                )}
        </Button>
        <Button
          disabled={isSaving}
          onClick={onCancel}
          size="sm"
          variant="ghost"
        >
          {t("credential_composer.actions.cancel", "Cancel")}
        </Button>
      </div>
    </>
  );

  const handleDraftFieldChange = (
    field: keyof RepositoryCredentialDraft,
    value: string,
  ) => {
    setDraft((current) => ({
      ...current,
      [field]: value,
    }));
    setErrors((current) => ({
      ...current,
      [field]: undefined,
    }));
  };

  const handleSave = async () => {
    const nextErrors = validateRepositoryCredentialDraft(draft, t);
    if (hasRepositoryCredentialDraftErrors(nextErrors)) {
      setErrors(nextErrors);
      return;
    }

    await onSave(buildSaveSecretCredentialInput(draft));
  };

  if (!renderSurface) {
    return content;
  }

  return (
    <SurfacePanel
      description={panelDescription}
      tone="inset"
      title={panelTitle}
    >
      {content}
    </SurfacePanel>
  );
}

function validateRepositoryCredentialDraft(
  draft: RepositoryCredentialDraft,
  t: Translate,
): RepositoryCredentialDraftErrors {
  const errors: RepositoryCredentialDraftErrors = {};

  if (!draft.name.trim()) {
    errors.name = t(
      "credential_composer.validation.name_required",
      "Credential name is required.",
    );
  }

  if (draft.kind === "git-http-basic") {
    if (!draft.username.trim()) {
      errors.username = t(
        "credential_composer.validation.username_required",
        "Username is required.",
      );
    }

    if (!draft.password.trim()) {
      errors.password = t(
        "credential_composer.validation.password_required",
        "Password or token is required.",
      );
    }
  }

  if (draft.kind === "git-http-bearer" && !draft.token.trim()) {
    errors.token = t(
      "credential_composer.validation.token_required",
      "Bearer token is required.",
    );
  }

  if (draft.kind === "itch-api-key" && !draft.apiKey.trim()) {
    errors.apiKey = t(
      "credential_composer.validation.api_key_required",
      "API key is required.",
    );
  }

  return errors;
}

function hasRepositoryCredentialDraftErrors(
  errors: RepositoryCredentialDraftErrors,
) {
  return Object.values(errors).some(Boolean);
}

function buildSaveSecretCredentialInput(
  draft: RepositoryCredentialDraft,
): SaveSecretCredentialInput {
  return {
    credential_id: draft.credentialId,
    config_json:
      draft.kind === "git-http-basic"
        ? JSON.stringify({
            password: draft.password.trim(),
            username: draft.username.trim(),
          })
        : draft.kind === "itch-api-key"
          ? JSON.stringify({
              api_key: draft.apiKey.trim(),
            })
          : JSON.stringify({
              token: draft.token.trim(),
            }),
    kind: draft.kind,
    name: draft.name.trim(),
  };
}

function buildRepositoryCredentialKindOptions(t: Translate) {
  return [
    {
      label: t("credential_composer.kind.http_basic", "HTTP basic"),
      value: "git-http-basic",
    },
    {
      label: t("credential_composer.kind.bearer_token", "Bearer token"),
      value: "git-http-bearer",
    },
  ] as const;
}

function buildPublishCredentialKindOptions(t: Translate) {
  return [
    {
      label: t("credential_composer.kind.itch_api_key", "Itch API key"),
      value: "itch-api-key",
    },
  ] as const;
}
