import { useState } from "react";

import FullScreenModal from "../FullScreenModal";
import {
  RepositoryCredentialComposer,
  type RepositoryCredentialInitialValue,
} from "../RepositoryCredentialComposer";
import { useLocalization, type Translate } from "../../LocalizationProvider";
import type { SaveSecretCredentialInput } from "../../services/projects";

type CredentialComposerModalProps = {
  initialCredential?: RepositoryCredentialInitialValue | null;
  onResolve?: (value?: SaveSecretCredentialInput | null) => void;
  onSubmit?: (input: SaveSecretCredentialInput) => Promise<void> | void;
  providerLabel: string;
  scope?: "repository" | "publish";
};

export function CredentialComposerModal({
  initialCredential = null,
  onResolve,
  onSubmit,
  providerLabel,
  scope = "repository",
}: CredentialComposerModalProps) {
  const { t } = useLocalization();
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const isEditing = initialCredential?.credentialId != null;
  const title =
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
  const description =
    scope === "publish"
      ? isEditing
        ? t(
            "credential_composer.publish.modal_edit_description",
            "Update one reusable {{providerLabel}} credential and replace the stored secret used by publish destinations.",
            { providerLabel },
          )
        : t(
            "credential_composer.publish.modal_create_description",
            "Create one reusable {{providerLabel}} credential and bind it to the selected publish destination.",
            { providerLabel },
          )
      : isEditing
        ? t(
            "credential_composer.repository.modal_edit_description",
            "Update one reusable {{providerLabel}} credential and replace the stored secret used by repository flows.",
            { providerLabel },
          )
        : t(
            "credential_composer.repository.modal_create_description",
            "Create one reusable {{providerLabel}} credential and connect it to the selected project.",
            { providerLabel },
          );

  const handleSave = async (input: SaveSecretCredentialInput) => {
    setIsSaving(true);
    setSaveError(null);

    try {
      await onSubmit?.(input);
      onResolve?.(input);
    } catch (error) {
      setSaveError(buildCredentialComposerErrorMessage(t, error));
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
        initialCredential={initialCredential}
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

function buildCredentialComposerErrorMessage(
  t: Translate,
  error: unknown,
) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t(
    "credential_composer.error.save_failed",
    "The desktop shell could not save the credential.",
  );
}

export default CredentialComposerModal;
