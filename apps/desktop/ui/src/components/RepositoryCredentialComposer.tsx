import { useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { SurfacePanel } from "./Surface";
import type {
  SaveSecretCredentialInput,
  SecretCredentialKind,
} from "../services/projects";

type RepositoryCredentialComposerProps = {
  isSaving: boolean;
  onCancel: () => void;
  onSave: (input: SaveSecretCredentialInput) => Promise<void> | void;
  providerLabel: string;
  saveError: string | null;
};

type RepositoryCredentialDraftKind = Exclude<
  SecretCredentialKind,
  "git-http-github-host-login"
>;

type RepositoryCredentialDraft = {
  kind: RepositoryCredentialDraftKind;
  name: string;
  password: string;
  token: string;
  username: string;
};

type RepositoryCredentialDraftErrors = {
  name?: string;
  password?: string;
  token?: string;
  username?: string;
};

const CREDENTIAL_KIND_OPTIONS = [
  { label: "HTTP basic", value: "git-http-basic" },
  { label: "Bearer token", value: "git-http-bearer" },
] as const;

export function RepositoryCredentialComposer({
  isSaving,
  onCancel,
  onSave,
  providerLabel,
  saveError,
}: RepositoryCredentialComposerProps) {
  const [draft, setDraft] = useState<RepositoryCredentialDraft>({
    kind: "git-http-basic",
    name: "",
    password: "",
    token: "",
    username: "",
  });
  const [errors, setErrors] = useState<RepositoryCredentialDraftErrors>({});

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

  return (
    <SurfacePanel
      description={`Create one reusable ${providerLabel} credential and connect it to this project.`}
      tone="inset"
      title="New repository credential"
    >
      <TextField
        autoComplete="off"
        error={errors.name}
        hint="Use a unique reusable credential name."
        label="Credential name"
        onChange={(event) =>
          handleDraftFieldChange("name", event.currentTarget.value)
        }
        placeholder={`${providerLabel} repository credential`}
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
        options={CREDENTIAL_KIND_OPTIONS}
        value={draft.kind}
      />

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
      ) : (
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
          {isSaving ? "Saving credential..." : "Save credential"}
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
    credential_id: null,
    config_json:
      draft.kind === "git-http-basic"
        ? JSON.stringify({
            password: draft.password.trim(),
            username: draft.username.trim(),
          })
        : JSON.stringify({
            token: draft.token.trim(),
          }),
    kind: draft.kind,
    name: draft.name.trim(),
  };
}