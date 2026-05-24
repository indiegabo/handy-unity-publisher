import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { SurfacePanel } from "./Surface";
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

const REPOSITORY_CREDENTIAL_KIND_OPTIONS = [
  { label: "HTTP basic", value: "git-http-basic" },
  { label: "Bearer token", value: "git-http-bearer" },
] as const;

const PUBLISH_CREDENTIAL_KIND_OPTIONS = [
  { label: "Itch API key", value: "itch-api-key" },
] as const;

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
      ? PUBLISH_CREDENTIAL_KIND_OPTIONS
      : REPOSITORY_CREDENTIAL_KIND_OPTIONS;
  const panelTitle =
    scope === "publish"
      ? isEditing
        ? "Edit publish credential"
        : "New publish credential"
      : isEditing
        ? "Edit repository credential"
        : "New repository credential";
  const panelDescription =
    scope === "publish"
      ? isEditing
        ? `Update one reusable ${providerLabel} credential and replace the stored secret used by publish destinations.`
        : `Create one reusable ${providerLabel} credential and bind it to this publish destination.`
      : isEditing
        ? `Update one reusable ${providerLabel} credential and replace the stored secret used by repository flows.`
        : `Create one reusable ${providerLabel} credential and connect it to this project.`;
  const credentialNamePlaceholder =
    scope === "publish"
      ? `${providerLabel} publish credential`
      : `${providerLabel} repository credential`;

  const content = (
    <>
      <TextField
        autoComplete="off"
        data-overlay-autofocus
        error={errors.name}
        hint="Use a unique reusable credential name."
        label="Credential name"
        onChange={(event) =>
          handleDraftFieldChange("name", event.currentTarget.value)
        }
        placeholder={credentialNamePlaceholder}
        value={draft.name}
      />

      <SelectField
        label="Credential type"
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
          Re-enter the secret material to replace the stored credential value.
        </p>
      ) : null}

      {draft.kind === "git-http-basic" ? (
        <>
          <TextField
            autoComplete="username"
            error={errors.username}
            hint="The Git host username or service account login."
            label="Username"
            onChange={(event) =>
              handleDraftFieldChange("username", event.currentTarget.value)
            }
            value={draft.username}
          />
          <TextField
            autoComplete="new-password"
            error={errors.password}
            hint="Stored exactly as provided for HTTP basic authentication."
            label="Password or token"
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
          hint="Stored exactly as provided for bearer authorization."
          label="Bearer token"
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
          hint="Stored exactly as provided for the Itch butler API key."
          label="API key"
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
              ? "Saving changes..."
              : "Saving credential..."
            : isEditing
              ? "Save changes"
              : "Save credential"}
        </Button>
        <Button
          disabled={isSaving}
          onClick={onCancel}
          size="sm"
          variant="ghost"
        >
          Cancel
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
    const nextErrors = validateRepositoryCredentialDraft(draft);
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
): RepositoryCredentialDraftErrors {
  const errors: RepositoryCredentialDraftErrors = {};

  if (!draft.name.trim()) {
    errors.name = "Credential name is required.";
  }

  if (draft.kind === "git-http-basic") {
    if (!draft.username.trim()) {
      errors.username = "Username is required.";
    }

    if (!draft.password.trim()) {
      errors.password = "Password or token is required.";
    }
  }

  if (draft.kind === "git-http-bearer" && !draft.token.trim()) {
    errors.token = "Bearer token is required.";
  }

  if (draft.kind === "itch-api-key" && !draft.apiKey.trim()) {
    errors.apiKey = "API key is required.";
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
