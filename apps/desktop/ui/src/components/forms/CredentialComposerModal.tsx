import { useState } from "react";

import FullScreenModal from "../FullScreenModal";
import { RepositoryCredentialComposer } from "../RepositoryCredentialComposer";
import type { SaveSecretCredentialInput } from "../../services/projects";

type CredentialComposerModalProps = {
  onResolve?: (value?: SaveSecretCredentialInput | null) => void;
  onSubmit?: (input: SaveSecretCredentialInput) => Promise<void> | void;
  providerLabel: string;
  scope?: "repository" | "publish";
};

export function CredentialComposerModal({
  onResolve,
  onSubmit,
  providerLabel,
  scope = "repository",
}: CredentialComposerModalProps) {
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const title =
    scope === "publish"
      ? "New publish credential"
      : "New repository credential";
  const description =
    scope === "publish"
      ? `Create one reusable ${providerLabel} credential and bind it to the selected publish destination.`
      : `Create one reusable ${providerLabel} credential and connect it to the selected project.`;

  const handleSave = async (input: SaveSecretCredentialInput) => {
    setIsSaving(true);
    setSaveError(null);

    try {
      await onSubmit?.(input);
      onResolve?.(input);
    } catch (error) {
      setSaveError(buildCredentialComposerErrorMessage(error));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <FullScreenModal
      description={description}
      dismissible={!isSaving}
      onResolve={onResolve}
      title={title}
    >
      <RepositoryCredentialComposer
        isSaving={isSaving}
        onCancel={() => onResolve?.(null)}
        onSave={handleSave}
        providerLabel={providerLabel}
        renderSurface={false}
        saveError={saveError}
        scope={scope}
      />
    </FullScreenModal>
  );
}

function buildCredentialComposerErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not save the credential.";
}

export default CredentialComposerModal;
