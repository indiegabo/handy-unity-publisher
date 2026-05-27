import { useState, type ReactElement } from "react";

import { Button, IconButton } from "./Button";
import { SelectField, TextField, type SelectOption } from "./Field";
import FullScreenModal from "./FullScreenModal";
import { type Translate, useLocalization } from "../LocalizationProvider";
import { useOverlay } from "./OverlayManager";
import { PathPickerField } from "./PathPickerField";
import { RepositoryCredentialComposer } from "./RepositoryCredentialComposer";
import SelectListFullScreen, {
  type SelectListItem,
} from "./SelectListFullScreen";
import {
  Badge,
  MetaItem,
  MetaRow,
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import type {
  CreateRepositoryProjectPublishTargetInput,
  RepositoryPublishTargetInspection,
  SaveSecretCredentialInput,
  SecretCredentialSetting,
  UpdateRepositoryProjectPublishTargetInput,
} from "../services/projects";

export type ProjectBuildTargetReference = {
  id: string;
  buildTargetId: number | null;
  name: string;
};

export type PublishDestinationKind = "filesystem" | "itch";

export type PublishDestinationBindingDraft = {
  id: string;
  buildTargetDraftId: string;
  buildTargetId: number | null;
  buildTargetName: string;
  enabled: boolean;
  filesystemDirectoryPath: string;
  itchChannel: string;
  itchUserversionTemplate: string;
};

export type PublishDestinationDraft = {
  id: string;
  publishTargetId: number | null;
  name: string;
  kind: PublishDestinationKind;
  enabled: boolean;
  itchAccountName: string;
  itchGameSlug: string;
  credentialsId: number | null;
  credentialsName: string | null;
  bindings: PublishDestinationBindingDraft[];
};

export type PublishDestinationBindingErrors = {
  buildTarget?: string;
  filesystemDirectoryPath?: string;
  itchChannel?: string;
};

export type PublishDestinationDraftErrors = {
  name?: string;
  credentialsId?: string;
  itchAccountName?: string;
  itchGameSlug?: string;
  bindingsRoot?: string;
  bindings: Record<string, PublishDestinationBindingErrors>;
};

export type PublishDestinationValidationErrors = {
  root?: string;
  destinations: Record<string, PublishDestinationDraftErrors>;
};

export type PublishDestinationReviewSummary = {
  id: string;
  name: string;
  kindLabel: string;
  enabled: boolean;
  bindingTargetNames: string[];
  missingCredential: boolean;
};

export type PublishDestinationEditingMode = "inline" | "overlay";
export type PublishCredentialSaveResult = number | null | void;

type PublishCredentialSelection = {
  credentialId: number;
  credentialName: string;
};

type PublishDestinationsEditorProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  destinations: PublishDestinationDraft[];
  disabled?: boolean;
  editingMode?: PublishDestinationEditingMode;
  errors?: PublishDestinationValidationErrors;
  onChange: (next: PublishDestinationDraft[]) => void;
  showItchUserversionTemplate?: boolean;
  onSaveCredential?: (
    destinationId: string,
    input: SaveSecretCredentialInput,
  ) => Promise<PublishCredentialSaveResult> | PublishCredentialSaveResult;
};

const DESTINATION_STATUS_OPTIONS = [
  { label: "Enabled", value: "enabled" },
  { label: "Disabled", value: "disabled" },
] as const;

const BINDING_STATUS_OPTIONS = [
  { label: "Enabled", value: "enabled" },
  { label: "Disabled", value: "disabled" },
] as const;
const BINDING_SELECTOR_OVERLAY_THRESHOLD = 8;
const ITCH_CHANNEL_EXAMPLE_PLACEHOLDER = "WebGL, Windows, Linux";

export function PublishDestinationsEditor({
  buildTargets,
  credentials,
  destinations,
  disabled = false,
  editingMode = "inline",
  errors,
  onChange,
  showItchUserversionTemplate = true,
  onSaveCredential,
}: PublishDestinationsEditorProps) {
  const { t } = useLocalization();
  const { openOverlay } = useOverlay();
  const [showDestinationMenu, setShowDestinationMenu] = useState(false);
  const [pendingDestinationRemovalId, setPendingDestinationRemovalId] =
    useState<string | null>(null);
  const [pendingBindingTargetSelections, setPendingBindingTargetSelections] =
    useState<Record<string, string>>({});
  const availableDestinationKinds = listPublishDestinationKinds().filter(
    (kind) => !destinations.some((destination) => destination.kind === kind),
  );

  const handleAddDestination = (kind: PublishDestinationKind) => {
    if (destinations.some((destination) => destination.kind === kind)) {
      return;
    }

    onChange([...destinations, createEmptyPublishDestinationDraft(kind)]);
    setShowDestinationMenu(false);
  };

  const handleDestinationChange = (
    destinationId: string,
    patch: Partial<PublishDestinationDraft>,
  ) => {
    onChange(
      destinations.map((destination) => {
        if (destination.id !== destinationId) {
          return destination;
        }

        const nextDestination = {
          ...destination,
          ...patch,
        };

        if (patch.kind === "filesystem") {
          nextDestination.credentialsId = null;
          nextDestination.credentialsName = null;
        }

        return nextDestination;
      }),
    );
  };

  const handleBindingChange = (
    destinationId: string,
    bindingId: string,
    patch: Partial<PublishDestinationBindingDraft>,
  ) => {
    onChange(
      destinations.map((destination) =>
        destination.id === destinationId
          ? {
              ...destination,
              bindings: destination.bindings.map((binding) =>
                binding.id === bindingId ? { ...binding, ...patch } : binding,
              ),
            }
          : destination,
      ),
    );
  };

  const handleDestinationRemovalRequest = (destinationId: string) => {
    const destination = destinations.find(
      (entry) => entry.id === destinationId,
    );
    if (!destination) {
      return;
    }

    if (destination.bindings.length === 0) {
      onChange(destinations.filter((entry) => entry.id !== destinationId));
      return;
    }

    setPendingDestinationRemovalId(destinationId);
  };

  const handleConfirmDestinationRemoval = () => {
    if (!pendingDestinationRemovalId) {
      return;
    }

    onChange(
      destinations.filter(
        (destination) => destination.id !== pendingDestinationRemovalId,
      ),
    );
    setPendingDestinationRemovalId(null);
  };

  const handleOpenCredentialComposer = async (destinationId: string) => {
    if (disabled || !onSaveCredential) {
      return;
    }

    await openOverlay<PublishCredentialSelection>(
      PublishCredentialComposerOverlay,
      {
        destinationId,
        onSubmit: onSaveCredential,
      },
    );
  };

  const handleOpenDestinationCreateOverlay = async (
    kind: PublishDestinationKind,
  ) => {
    if (disabled || !availableDestinationKinds.includes(kind)) {
      return;
    }

    setShowDestinationMenu(false);

    const createdDestination = await openOverlay<PublishDestinationDraft>(
      PublishDestinationEditorOverlay,
      {
        buildTargets,
        credentials,
        existingDestinations: destinations,
        initialDestination: createEmptyPublishDestinationDraft(kind),
        mode: "create",
        showItchUserversionTemplate,
        onSaveCredential,
      },
    );

    if (!createdDestination) {
      return;
    }

    onChange([...destinations, createdDestination]);
  };

  const handleOpenDestinationEditOverlay = async (destinationId: string) => {
    if (disabled) {
      return;
    }

    const currentDestination = destinations.find(
      (destination) => destination.id === destinationId,
    );
    if (!currentDestination) {
      return;
    }

    const updatedDestination = await openOverlay<PublishDestinationDraft>(
      PublishDestinationEditorOverlay,
      {
        buildTargets,
        credentials,
        existingDestinations: destinations.filter(
          (destination) => destination.id !== destinationId,
        ),
        initialDestination: clonePublishDestinationDraft(currentDestination),
        mode: "edit",
        showItchUserversionTemplate,
        onSaveCredential,
      },
    );

    if (!updatedDestination) {
      return;
    }

    onChange(
      destinations.map((destination) =>
        destination.id === destinationId ? updatedDestination : destination,
      ),
    );
  };

  const handleAddBinding = (
    destinationId: string,
    targetDraftId?: string,
    patch?: Partial<PublishDestinationBindingDraft>,
  ) => {
    const destination = destinations.find(
      (entry) => entry.id === destinationId,
    );
    if (!destination) {
      return;
    }

    const selectedTargetDraftId =
      targetDraftId ??
      resolvePendingBindingTargetId(
        destination,
        buildTargets,
        pendingBindingTargetSelections[destinationId],
      );
    const selectedTarget = buildTargets.find(
      (target) => target.id === selectedTargetDraftId,
    );
    if (!selectedTarget) {
      return;
    }

    if (
      destination.bindings.some(
        (binding) => binding.buildTargetDraftId === selectedTarget.id,
      )
    ) {
      return;
    }

    onChange(
      destinations.map((entry) =>
        entry.id === destinationId
          ? {
              ...entry,
              bindings: [
                ...entry.bindings,
                {
                  ...createEmptyPublishDestinationBindingDraft(selectedTarget),
                  ...(patch ?? {}),
                },
              ],
            }
          : entry,
      ),
    );
    setPendingBindingTargetSelections((current) => ({
      ...current,
      [destinationId]: "",
    }));
  };

  const handleRemoveBinding = (destinationId: string, bindingId: string) => {
    onChange(
      destinations.map((destination) =>
        destination.id === destinationId
          ? {
              ...destination,
              bindings: destination.bindings.filter(
                (binding) => binding.id !== bindingId,
              ),
            }
          : destination,
      ),
    );
  };

  const pendingDestinationRemoval = pendingDestinationRemovalId
    ? (destinations.find(
        (destination) => destination.id === pendingDestinationRemovalId,
      ) ?? null)
    : null;
  const pendingRemovalTargets = pendingDestinationRemoval
    ? collectPublishDestinationBindingTargets(
        pendingDestinationRemoval,
        buildTargets,
      )
    : [];

  return (
    <div className="project-detail-target-list">
      <div className="publish-destination-toolbar">
        {editingMode === "overlay" ? (
          <div className="publish-destination-menu">
            <Button
              disabled={disabled || availableDestinationKinds.length === 0}
              leadingIcon="plus"
              onClick={() => setShowDestinationMenu((current) => !current)}
              size="sm"
              variant="secondary"
            >
              {t(
                "publish_destinations.editor.actions.add_destination",
                "Add destination",
              )}
            </Button>
            {showDestinationMenu ? (
              <div
                aria-label={t(
                  "publish_destinations.editor.menu.label",
                  "Destination list",
                )}
                className="publish-destination-menu__popover"
                role="menu"
              >
                {listPublishDestinationKinds().map((kind) => {
                  const alreadyAdded = destinations.some(
                    (destination) => destination.kind === kind,
                  );

                  return (
                    <button
                      className="publish-destination-menu__item"
                      disabled={disabled || alreadyAdded}
                      key={kind}
                      onClick={() => {
                        void handleOpenDestinationCreateOverlay(kind);
                      }}
                      type="button"
                    >
                      {formatPublishDestinationKindLabel(t, kind)}
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
        ) : (
          <div className="publish-destination-menu">
            <Button
              disabled={disabled}
              leadingIcon="plus"
              onClick={() => setShowDestinationMenu((current) => !current)}
              size="sm"
              variant="secondary"
            >
              {t(
                "publish_destinations.editor.actions.add_destination",
                "Add destination",
              )}
            </Button>
            {showDestinationMenu ? (
              <div
                aria-label={t(
                  "publish_destinations.editor.menu.label",
                  "Destination list",
                )}
                className="publish-destination-menu__popover"
                role="menu"
              >
                {listPublishDestinationKinds().map((kind) => {
                  const alreadyAdded = destinations.some(
                    (destination) => destination.kind === kind,
                  );

                  return (
                    <button
                      className="publish-destination-menu__item"
                      disabled={disabled || alreadyAdded}
                      key={kind}
                      onClick={() => handleAddDestination(kind)}
                      type="button"
                    >
                      {formatPublishDestinationKindLabel(t, kind)}
                    </button>
                  );
                })}
              </div>
            ) : null}
          </div>
        )}
      </div>

      {errors?.root ? (
        <p className="feed-banner feed-banner--error">{errors.root}</p>
      ) : null}

      {pendingDestinationRemoval ? (
        <div className="wizard-callout wizard-callout--compact wizard-callout--auth">
          <div className="wizard-callout__header">
            <div>
              <p className="wizard-callout__title">
                {t(
                  "publish_destinations.editor.confirm_remove.title",
                  "Confirm destination removal",
                )}
              </p>
              <p className="wizard-callout__copy">
                {t(
                  "publish_destinations.editor.confirm_remove.copy",
                  "Removing {{destinationTitle}} also removes persisted bindings for {{bindingTargets}}.",
                  {
                    bindingTargets:
                      pendingRemovalTargets.join(", ") ||
                      t(
                        "publish_destinations.editor.confirm_remove.binding_fallback",
                        "its bound build targets",
                      ),
                    destinationTitle: formatPublishDestinationTitle(
                      t,
                      pendingDestinationRemoval,
                    ),
                  },
                )}
              </p>
            </div>
          </div>

          <div className="wizard-callout__actions">
            <Button
              disabled={disabled}
              leadingIcon="trash"
              onClick={handleConfirmDestinationRemoval}
              size="sm"
              variant="primary"
            >
              {t(
                "publish_destinations.editor.confirm_remove.confirm",
                "Remove destination",
              )}
            </Button>
            <Button
              disabled={disabled}
              onClick={() => setPendingDestinationRemovalId(null)}
              size="sm"
              variant="ghost"
            >
              {t("publish_destinations.editor.confirm_remove.cancel", "Cancel")}
            </Button>
          </div>
        </div>
      ) : null}

      {destinations.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "publish_destinations.editor.empty_state",
              "No publish destinations configured.",
            )}
          </p>
        </div>
      ) : null}

      {destinations.map((destination, index) => {
        const destinationErrors =
          errors?.destinations[destination.id] ??
          createEmptyPublishDestinationErrors();
        const pendingBindingTargetId = resolvePendingBindingTargetId(
          destination,
          buildTargets,
          pendingBindingTargetSelections[destination.id],
        );
        const bindingTargetNames = collectPublishDestinationBindingTargets(
          destination,
          buildTargets,
        );
        const adapter = getPublishDestinationAdapter(destination.kind);
        const AdapterComponent = adapter.Component;

        if (editingMode === "overlay") {
          const destinationErrorPreview =
            formatPublishDestinationErrorPreview(destinationErrors);

          return (
            <SurfacePanel
              actions={
                <div className="publish-destination-quick-view__actions">
                  <Button
                    disabled={disabled}
                    onClick={() => {
                      void handleOpenDestinationEditOverlay(destination.id);
                    }}
                    size="sm"
                    variant="ghost"
                  >
                    {t("publish_destinations.editor.actions.edit", "Edit")}
                  </Button>
                  <Button
                    disabled={disabled}
                    leadingIcon="trash"
                    onClick={() =>
                      handleDestinationRemovalRequest(destination.id)
                    }
                    size="sm"
                    variant="ghost"
                  >
                    {t("publish_destinations.editor.actions.remove", "Remove")}
                  </Button>
                </div>
              }
              className="publish-destination-quick-view"
              key={destination.id}
              summary={
                <MetaRow className="wizard-target-card__summary">
                  <MetaItem
                    label={t(
                      "publish_destinations.editor.meta.status",
                      "Status",
                    )}
                  >
                    {destination.enabled
                      ? t("publish_destinations.editor.meta.enabled", "Enabled")
                      : t(
                          "publish_destinations.editor.meta.disabled",
                          "Disabled",
                        )}
                  </MetaItem>
                  <MetaItem
                    label={t(
                      "publish_destinations.editor.meta.bindings",
                      "Bindings",
                    )}
                  >
                    {formatPublishDestinationBindingCount(
                      t,
                      bindingTargetNames.length,
                    )}
                  </MetaItem>
                  <MetaItem
                    label={t(
                      "publish_destinations.editor.meta.targets",
                      "Targets",
                    )}
                  >
                    {formatPublishDestinationBindingPreview(
                      t,
                      bindingTargetNames,
                    )}
                  </MetaItem>
                  <MetaItem
                    label={
                      destination.kind === "itch"
                        ? t(
                            "publish_destinations.editor.meta.credential",
                            "Credential",
                          )
                        : t("publish_destinations.editor.meta.mode", "Mode")
                    }
                  >
                    {formatPublishDestinationOperationalSummary(t, destination)}
                  </MetaItem>
                </MetaRow>
              }
              title={formatPublishDestinationTitle(t, destination)}
              tone="inset"
            >
              {destinationErrorPreview ? (
                <p className="ui-field__error">{destinationErrorPreview}</p>
              ) : (
                <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                  {buildPublishDestinationQuickViewCopy(
                    t,
                    destination,
                    bindingTargetNames,
                  )}
                </p>
              )}
            </SurfacePanel>
          );
        }

        return (
          <VerticalAccordion
            bodyClassName="wizard-target-card__body"
            bodyInset
            className="wizard-target-card"
            defaultOpen={index === 0}
            header={
              <div className="wizard-target-card__header">
                <div className="wizard-target-card__top-row">
                  <IconButton
                    className="wizard-target-card__remove"
                    disabled={disabled}
                    icon="trash"
                    label={t(
                      "publish_destinations.editor.actions.remove_destination_with_kind",
                      "Remove {{kind}} destination",
                      { kind: adapter.label },
                    )}
                    onClick={() =>
                      handleDestinationRemovalRequest(destination.id)
                    }
                    size="sm"
                    variant="ghost"
                  />
                </div>

                <div className="wizard-target-card__title-block">
                  <h3 className="wizard-target-card__title">{adapter.label}</h3>
                </div>

                <div className="wizard-target-card__badges">
                  <Badge tone={destination.enabled ? "strong" : "muted"}>
                    {destination.enabled
                      ? t(
                          "publish_destinations.editor.meta.enabled_lower",
                          "enabled",
                        )
                      : t(
                          "publish_destinations.editor.meta.disabled_lower",
                          "disabled",
                        )}
                  </Badge>
                </div>

                <SummaryStrip className="publish-destination-card__summary-strip">
                  <MetaRow className="wizard-target-card__summary">
                    <MetaItem
                      label={t(
                        "publish_destinations.editor.meta.bindings",
                        "Bindings",
                      )}
                    >
                      {formatPublishDestinationBindingCount(
                        t,
                        bindingTargetNames.length,
                      )}
                    </MetaItem>
                    <MetaItem
                      label={t(
                        "publish_destinations.editor.meta.targets",
                        "Targets",
                      )}
                    >
                      {formatPublishDestinationBindingPreview(
                        t,
                        bindingTargetNames,
                      )}
                    </MetaItem>
                    <MetaItem
                      label={
                        destination.kind === "itch"
                          ? t(
                              "publish_destinations.editor.meta.credential",
                              "Credential",
                            )
                          : t("publish_destinations.editor.meta.mode", "Mode")
                      }
                    >
                      {formatPublishDestinationOperationalSummary(
                        t,
                        destination,
                      )}
                    </MetaItem>
                  </MetaRow>
                </SummaryStrip>
              </div>
            }
            headerSeparated
            key={destination.id}
            tone="section"
            triggerMode="button"
          >
            <AdapterComponent
              buildTargets={buildTargets}
              credentials={credentials}
              destination={destination}
              destinationErrors={destinationErrors}
              disabled={disabled}
              preferBindingCreateOverlay={false}
              onAddBinding={() => handleAddBinding(destination.id)}
              onBindingChange={(bindingId, patch) =>
                handleBindingChange(destination.id, bindingId, patch)
              }
              onDestinationChange={(patch) =>
                handleDestinationChange(destination.id, patch)
              }
              onOpenCredentialComposer={() => {
                void handleOpenCredentialComposer(destination.id);
              }}
              onPendingBindingTargetChange={(nextTargetId) =>
                setPendingBindingTargetSelections((current) => ({
                  ...current,
                  [destination.id]: nextTargetId,
                }))
              }
              onRemoveBinding={(bindingId) =>
                handleRemoveBinding(destination.id, bindingId)
              }
              pendingBindingTargetId={pendingBindingTargetId}
              showItchUserversionTemplate={showItchUserversionTemplate}
              showDestinationStatus
            />
          </VerticalAccordion>
        );
      })}
    </div>
  );
}

type PublishDestinationEditorMode = "create" | "edit";

type PublishDestinationEditorOverlayProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  existingDestinations: PublishDestinationDraft[];
  initialDestination: PublishDestinationDraft;
  mode: PublishDestinationEditorMode;
  onResolve?: (value?: PublishDestinationDraft | null) => void;
  showItchUserversionTemplate?: boolean;
  onSaveCredential?: (
    destinationId: string,
    input: SaveSecretCredentialInput,
  ) => Promise<PublishCredentialSaveResult> | PublishCredentialSaveResult;
};

function PublishDestinationEditorOverlay({
  buildTargets,
  credentials,
  existingDestinations,
  initialDestination,
  mode,
  onResolve,
  showItchUserversionTemplate = true,
  onSaveCredential,
}: PublishDestinationEditorOverlayProps) {
  const { t } = useLocalization();
  const { openOverlay } = useOverlay();
  const [draft, setDraft] = useState(() =>
    clonePublishDestinationDraft(initialDestination),
  );
  const [pendingBindingTargetSelection, setPendingBindingTargetSelection] =
    useState("");
  const [attemptedSave, setAttemptedSave] = useState(false);
  const validationErrors = validatePublishDestinationDrafts(
    [...existingDestinations, draft],
    buildTargets,
  );
  const destinationErrors = attemptedSave
    ? (validationErrors.destinations[draft.id] ??
      createEmptyPublishDestinationErrors())
    : createEmptyPublishDestinationErrors();
  const pendingBindingTargetId = resolvePendingBindingTargetId(
    draft,
    buildTargets,
    pendingBindingTargetSelection,
  );
  const adapter = getPublishDestinationAdapter(draft.kind);
  const AdapterComponent = adapter.Component;
  const isCreateMode = mode === "create";

  const handleDestinationChange = (patch: Partial<PublishDestinationDraft>) => {
    setDraft((current) => {
      const nextDraft = {
        ...current,
        ...patch,
      };

      if (patch.kind) {
        nextDraft.name = derivePublishDestinationName(patch.kind);
      }

      if (patch.kind === "filesystem") {
        nextDraft.credentialsId = null;
        nextDraft.credentialsName = null;
      }

      return nextDraft;
    });
  };

  const handleBindingChange = (
    bindingId: string,
    patch: Partial<PublishDestinationBindingDraft>,
  ) => {
    setDraft((current) => ({
      ...current,
      bindings: current.bindings.map((binding) =>
        binding.id === bindingId ? { ...binding, ...patch } : binding,
      ),
    }));
  };

  const handleAddBinding = (
    targetDraftId?: string,
    patch?: Partial<PublishDestinationBindingDraft>,
  ) => {
    const resolvedTargetDraftId = targetDraftId ?? pendingBindingTargetId;
    const selectedTarget = buildTargets.find(
      (target) => target.id === resolvedTargetDraftId,
    );
    if (!selectedTarget) {
      return;
    }

    if (
      draft.bindings.some(
        (binding) => binding.buildTargetDraftId === selectedTarget.id,
      )
    ) {
      return;
    }

    setDraft((current) => ({
      ...current,
      bindings: [
        ...current.bindings,
        {
          ...createEmptyPublishDestinationBindingDraft(selectedTarget),
          ...(patch ?? {}),
        },
      ],
    }));
    setPendingBindingTargetSelection("");
  };

  const handleRemoveBinding = (bindingId: string) => {
    setDraft((current) => ({
      ...current,
      bindings: current.bindings.filter((binding) => binding.id !== bindingId),
    }));
  };

  const handleOpenCredentialComposer = async () => {
    if (!onSaveCredential) {
      return;
    }

    const createdCredential = await openOverlay<PublishCredentialSelection>(
      PublishCredentialComposerOverlay,
      {
        destinationId: draft.id,
        onSubmit: onSaveCredential,
      },
    );

    if (createdCredential) {
      setDraft((current) => ({
        ...current,
        credentialsId: createdCredential.credentialId,
        credentialsName:
          resolveCredentialNameById(
            credentials,
            createdCredential.credentialId,
          ) || createdCredential.credentialName,
      }));
    }
  };

  const handleSave = () => {
    setAttemptedSave(true);

    if (hasPublishDestinationValidationErrors(validationErrors)) {
      return;
    }

    onResolve?.(draft);
  };

  return (
    <FullScreenModal
      className="publish-destination-editor-modal"
      description={
        isCreateMode
          ? undefined
          : t(
              "publish_destinations.editor.modal.edit_description",
              "Update the selected publish destination and return to the wizard when its delivery rules are ready.",
            )
      }
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
            {t("publish_destinations.editor.actions.cancel", "Cancel")}
          </Button>
          <Button
            leadingIcon="plus"
            onClick={handleSave}
            size="sm"
            variant="primary"
          >
            {isCreateMode
              ? t("publish_destinations.editor.actions.confirm", "Confirm")
              : t(
                  "publish_destinations.editor.actions.save_destination",
                  "Save destination",
                )}
          </Button>
        </div>
      }
      onResolve={onResolve}
      title={
        isCreateMode
          ? t(
              "publish_destinations.editor.modal.add_title",
              "Add {{kind}} destination",
              { kind: formatPublishDestinationKindLabel(t, draft.kind) },
            )
          : t(
              "publish_destinations.editor.modal.edit_title",
              "Edit {{kind}} destination",
              { kind: formatPublishDestinationKindLabel(t, draft.kind) },
            )
      }
    >
      {attemptedSave && validationErrors.root ? (
        <p className="feed-banner feed-banner--error">
          {validationErrors.root}
        </p>
      ) : null}

      <div className="project-detail-form-grid publish-destination-editor-modal__content">
        <AdapterComponent
          buildTargets={buildTargets}
          credentials={credentials}
          destination={draft}
          destinationErrors={destinationErrors}
          disabled={false}
          preferBindingCreateOverlay
          onAddBinding={handleAddBinding}
          onBindingChange={handleBindingChange}
          onDestinationChange={handleDestinationChange}
          onOpenCredentialComposer={() => {
            void handleOpenCredentialComposer();
          }}
          onPendingBindingTargetChange={setPendingBindingTargetSelection}
          onRemoveBinding={handleRemoveBinding}
          pendingBindingTargetId={pendingBindingTargetId}
          showItchUserversionTemplate={showItchUserversionTemplate}
          showDestinationStatus={!isCreateMode}
        />
      </div>
    </FullScreenModal>
  );
}

type PublishCredentialComposerOverlayProps = {
  destinationId: string;
  onResolve?: (value?: PublishCredentialSelection | null) => void;
  onSubmit?: (
    destinationId: string,
    input: SaveSecretCredentialInput,
  ) => Promise<PublishCredentialSaveResult> | PublishCredentialSaveResult;
};

function PublishCredentialComposerOverlay({
  destinationId,
  onResolve,
  onSubmit,
}: PublishCredentialComposerOverlayProps) {
  const { t } = useLocalization();
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const handleSave = async (input: SaveSecretCredentialInput) => {
    setIsSaving(true);
    setSaveError(null);

    try {
      const createdCredentialId = await onSubmit?.(destinationId, input);
      onResolve?.(
        typeof createdCredentialId === "number"
          ? {
              credentialId: createdCredentialId,
              credentialName: input.name.trim(),
            }
          : null,
      );
    } catch (error) {
      setSaveError(buildPublishCredentialComposerErrorMessage(error));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <FullScreenModal
      description={t(
        "publish_destinations.editor.credentials.composer.description",
        "Create one reusable Itch.io credential and bind it to the selected publish destination.",
      )}
      dismissible={!isSaving}
      onResolve={onResolve}
      title={t(
        "publish_destinations.editor.credentials.composer.title",
        "New publish credential",
      )}
    >
      <RepositoryCredentialComposer
        isSaving={isSaving}
        onCancel={() => onResolve?.(null)}
        onSave={handleSave}
        providerLabel={t(
          "publish_destinations.editor.credentials.provider",
          "Itch.io",
        )}
        renderSurface={false}
        saveError={saveError}
        scope="publish"
      />
    </FullScreenModal>
  );
}

type PublishDestinationAdapterComponentProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  destination: PublishDestinationDraft;
  destinationErrors: PublishDestinationDraftErrors;
  disabled: boolean;
  preferBindingCreateOverlay?: boolean;
  onAddBinding: (
    targetDraftId?: string,
    patch?: Partial<PublishDestinationBindingDraft>,
  ) => void;
  onBindingChange: (
    bindingId: string,
    patch: Partial<PublishDestinationBindingDraft>,
  ) => void;
  onDestinationChange: (patch: Partial<PublishDestinationDraft>) => void;
  onOpenCredentialComposer?: () => void;
  onPendingBindingTargetChange: (nextTargetId: string) => void;
  onRemoveBinding: (bindingId: string) => void;
  pendingBindingTargetId: string;
  showItchUserversionTemplate: boolean;
  showDestinationStatus: boolean;
};

type PublishDestinationAdapterDefinition = {
  Component: (props: PublishDestinationAdapterComponentProps) => ReactElement;
  kind: PublishDestinationKind;
  label: string;
};

type PublishDestinationBindingFieldRendererProps = {
  binding: PublishDestinationBindingDraft;
  bindingErrors: PublishDestinationBindingErrors;
  disabled: boolean;
  onBindingChange: (patch: Partial<PublishDestinationBindingDraft>) => void;
};

type PublishDestinationBindingsSectionProps = {
  autoFocusTargetSelector?: boolean;
  buildTargets: ProjectBuildTargetReference[];
  description?: string;
  destination: PublishDestinationDraft;
  destinationErrors: PublishDestinationDraftErrors;
  disabled: boolean;
  preferBindingCreateOverlay?: boolean;
  onAddBinding: (
    targetDraftId?: string,
    patch?: Partial<PublishDestinationBindingDraft>,
  ) => void;
  onBindingChange: (
    bindingId: string,
    patch: Partial<PublishDestinationBindingDraft>,
  ) => void;
  onPendingBindingTargetChange: (nextTargetId: string) => void;
  onRemoveBinding: (bindingId: string) => void;
  pendingBindingTargetId: string;
  renderBindingFields: (
    props: PublishDestinationBindingFieldRendererProps,
  ) => ReactElement;
  showItchUserversionTemplate?: boolean;
  showBindingStatus?: boolean;
  title?: string;
};

type PublishDestinationBindingEditorOverlayMode = "create" | "edit";

type PublishDestinationBindingEditorOverlayResult = {
  targetDraftId: string;
  patch: Partial<PublishDestinationBindingDraft>;
};

type PublishDestinationBindingEditorOverlayProps = {
  buildTargets: ProjectBuildTargetReference[];
  destinationKind: PublishDestinationKind;
  initialBinding: PublishDestinationBindingDraft;
  initialTargetDraftId: string;
  mode: PublishDestinationBindingEditorOverlayMode;
  onResolve?: (
    value?: PublishDestinationBindingEditorOverlayResult | null,
  ) => void;
  showItchUserversionTemplate?: boolean;
  showBindingStatus?: boolean;
};

function PublishDestinationBindingEditorOverlay({
  buildTargets,
  destinationKind,
  initialBinding,
  initialTargetDraftId,
  mode,
  onResolve,
  showItchUserversionTemplate = true,
  showBindingStatus = true,
}: PublishDestinationBindingEditorOverlayProps) {
  const { t } = useLocalization();
  const isCreateMode = mode === "create";
  const [targetDraftId, setTargetDraftId] =
    useState<string>(initialTargetDraftId);
  const [draft, setDraft] = useState<PublishDestinationBindingDraft>({
    ...initialBinding,
    itchChannel: initialBinding.itchChannel,
  });
  const [attemptedSave, setAttemptedSave] = useState(false);

  const targetError =
    attemptedSave && !targetDraftId
      ? t(
          "publish_destinations.editor.bindings.target_required",
          "Build target is required.",
        )
      : undefined;
  const directoryError =
    attemptedSave && destinationKind === "filesystem"
      ? !draft.filesystemDirectoryPath.trim()
        ? t(
            "publish_destinations.editor.filesystem.directory_required",
            "Destination directory is required.",
          )
        : !looksLikeAbsolutePath(draft.filesystemDirectoryPath)
          ? t(
              "publish_destinations.editor.filesystem.directory_absolute",
              "Destination directory must be an absolute path.",
            )
          : undefined
      : undefined;
  const itchChannelError =
    attemptedSave && destinationKind === "itch"
      ? !draft.itchChannel.trim()
        ? t(
            "publish_destinations.editor.itch.channel_required",
            "Itch channel is required.",
          )
        : undefined
      : undefined;

  const handleSave = () => {
    setAttemptedSave(true);

    if (targetError || directoryError || itchChannelError) {
      return;
    }

    const patch: Partial<PublishDestinationBindingDraft> = {
      enabled: draft.enabled,
      filesystemDirectoryPath: draft.filesystemDirectoryPath,
      itchChannel: draft.itchChannel.trim(),
      itchUserversionTemplate: draft.itchUserversionTemplate,
    };

    onResolve?.({
      patch,
      targetDraftId,
    });
  };

  return (
    <FullScreenModal
      description={
        isCreateMode
          ? t(
              "publish_destinations.editor.bindings.modal.create_description",
              "Configure one target binding and return to the destination editor with a compact summary card.",
            )
          : t(
              "publish_destinations.editor.bindings.modal.edit_description",
              "Update this target binding and return when delivery fields are ready.",
            )
      }
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button onClick={() => onResolve?.(null)} size="sm" variant="ghost">
            {t("publish_destinations.editor.actions.cancel", "Cancel")}
          </Button>
          <Button
            leadingIcon="plus"
            onClick={handleSave}
            size="sm"
            variant="primary"
          >
            {isCreateMode
              ? t("publish_destinations.editor.actions.confirm", "Confirm")
              : t(
                  "publish_destinations.editor.bindings.actions.save_binding",
                  "Save binding",
                )}
          </Button>
        </div>
      }
      onResolve={onResolve}
      title={
        isCreateMode
          ? t(
              "publish_destinations.editor.bindings.modal.add_title",
              "Add target binding",
            )
          : t(
              "publish_destinations.editor.bindings.modal.edit_title",
              "Edit target binding",
            )
      }
    >
      <div className="project-detail-form-grid publish-destination-editor-modal__content">
        {isCreateMode ? (
          <SelectField
            data-overlay-autofocus
            error={targetError}
            label={t("publish_destinations.editor.meta.target", "Target")}
            onChange={(event) => setTargetDraftId(event.currentTarget.value)}
            options={buildBindingTargetOptions(t, buildTargets)}
            value={targetDraftId}
          />
        ) : null}

        {showBindingStatus ? (
          <SelectField
            data-overlay-autofocus={!isCreateMode}
            label={t("publish_destinations.editor.meta.status", "Status")}
            onChange={(event) => {
              const nextStatus = event.currentTarget.value;

              setDraft((current) => ({
                ...current,
                enabled: nextStatus === "enabled",
              }));
            }}
            options={BINDING_STATUS_OPTIONS}
            value={draft.enabled ? "enabled" : "disabled"}
          />
        ) : null}

        {destinationKind === "filesystem" ? (
          <PathPickerField
            buttonLabel={t(
              "publish_destinations.editor.filesystem.pick_folder",
              "Pick folder",
            )}
            dialogTitle={t(
              "publish_destinations.editor.filesystem.dialog_title",
              "Select publish destination folder",
            )}
            error={directoryError}
            hint={t(
              "publish_destinations.editor.filesystem.directory_hint",
              "The artifact will move into this absolute directory when the binding succeeds.",
            )}
            label={t(
              "publish_destinations.editor.filesystem.directory_label",
              "Destination directory",
            )}
            onPathPicked={(path) =>
              setDraft((current) => ({
                ...current,
                filesystemDirectoryPath: path,
              }))
            }
            pickerKind="directory"
            placeholder={t(
              "publish_destinations.editor.filesystem.directory_placeholder",
              "D:/Published/Windows",
            )}
            value={draft.filesystemDirectoryPath}
          />
        ) : (
          <>
            <TextField
              error={itchChannelError}
              hint={t(
                "publish_destinations.editor.itch.channel_hint",
                "Use the Itch channel that should receive this build target artifact.",
              )}
              label={t(
                "publish_destinations.editor.itch.channel",
                "Itch channel",
              )}
              onChange={(event) => {
                const nextChannel = event.currentTarget.value;

                setDraft((current) => ({
                  ...current,
                  itchChannel: nextChannel,
                }));
              }}
              placeholder={ITCH_CHANNEL_EXAMPLE_PLACEHOLDER}
              value={draft.itchChannel}
            />

            {showItchUserversionTemplate ? (
              <TextField
                label={t(
                  "publish_destinations.editor.itch.userversion_template",
                  "Itch userversion template",
                )}
                onChange={(event) => {
                  const nextTemplate = event.currentTarget.value;

                  setDraft((current) => ({
                    ...current,
                    itchUserversionTemplate: nextTemplate,
                  }));
                }}
                placeholder="{{git_tag}}"
                value={draft.itchUserversionTemplate}
              />
            ) : null}
          </>
        )}
      </div>
    </FullScreenModal>
  );
}

function FilesystemPublishDestinationAdapter({
  buildTargets,
  destination,
  destinationErrors,
  disabled,
  preferBindingCreateOverlay = false,
  onAddBinding,
  onBindingChange,
  onDestinationChange,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
  showDestinationStatus,
}: PublishDestinationAdapterComponentProps) {
  const { t } = useLocalization();
  return (
    <>
      {showDestinationStatus ? (
        <SurfacePanel
          bodyClassName="wizard-form-grid"
          className="project-detail-form-grid__span-full"
          description={t(
            "publish_destinations.editor.filesystem.identity.description",
            "Configure the lifecycle state for this filesystem publish destination. Bound target paths stay with each individual binding.",
          )}
          headerSeparated
          title={t(
            "publish_destinations.editor.filesystem.identity.title",
            "Destination identity",
          )}
          tone="inset"
        >
          <SelectField
            data-overlay-autofocus
            label={t("publish_destinations.editor.meta.status", "Status")}
            onChange={(event) =>
              onDestinationChange({
                enabled: event.currentTarget.value === "enabled",
              })
            }
            options={DESTINATION_STATUS_OPTIONS}
            value={destination.enabled ? "enabled" : "disabled"}
          />
        </SurfacePanel>
      ) : null}

      <PublishDestinationBindingsSection
        autoFocusTargetSelector={!showDestinationStatus}
        buildTargets={buildTargets}
        destination={destination}
        destinationErrors={destinationErrors}
        disabled={disabled}
        preferBindingCreateOverlay={preferBindingCreateOverlay}
        onAddBinding={onAddBinding}
        onBindingChange={onBindingChange}
        onPendingBindingTargetChange={onPendingBindingTargetChange}
        onRemoveBinding={onRemoveBinding}
        pendingBindingTargetId={pendingBindingTargetId}
        renderBindingFields={({
          binding,
          bindingErrors,
          disabled: bindingDisabled,
          onBindingChange: handleBindingFieldChange,
        }) => (
          <PathPickerField
            buttonLabel={t(
              "publish_destinations.editor.filesystem.pick_folder",
              "Pick folder",
            )}
            dialogTitle={t(
              "publish_destinations.editor.filesystem.dialog_title",
              "Select publish destination folder",
            )}
            disabled={bindingDisabled}
            error={bindingErrors.filesystemDirectoryPath}
            hint={t(
              "publish_destinations.editor.filesystem.directory_hint",
              "The artifact will move into this absolute directory when the binding succeeds.",
            )}
            label={t(
              "publish_destinations.editor.filesystem.directory_label",
              "Destination directory",
            )}
            onPathPicked={(path) =>
              handleBindingFieldChange({
                filesystemDirectoryPath: path,
              })
            }
            pickerKind="directory"
            placeholder={t(
              "publish_destinations.editor.filesystem.directory_placeholder",
              "D:/Published/Windows",
            )}
            value={binding.filesystemDirectoryPath}
          />
        )}
        showBindingStatus={showDestinationStatus}
      />
    </>
  );
}

function ItchPublishDestinationAdapter({
  buildTargets,
  credentials,
  destination,
  destinationErrors,
  disabled,
  preferBindingCreateOverlay = false,
  onAddBinding,
  onBindingChange,
  onDestinationChange,
  onOpenCredentialComposer,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
  showItchUserversionTemplate,
  showDestinationStatus,
}: PublishDestinationAdapterComponentProps) {
  const { t } = useLocalization();
  const publishCredentialOptions = buildPublishCredentialOptions(
    credentials,
    destination.credentialsId,
    destination.credentialsName,
  );
  const showExpandedCopy = showDestinationStatus;

  return (
    <>
      <SurfacePanel
        bodyClassName="wizard-form-grid"
        className="project-detail-form-grid__span-full"
        description={
          showExpandedCopy
            ? t(
                "publish_destinations.editor.itch.identity.description",
                "Define the operator-facing identity and upload settings for this Itch publish destination.",
              )
            : undefined
        }
        headerSeparated
        title={
          showExpandedCopy
            ? t(
                "publish_destinations.editor.itch.identity.title",
                "Destination identity",
              )
            : t("publish_destinations.editor.itch.title", "Destination")
        }
        tone="inset"
      >
        {showDestinationStatus ? (
          <SelectField
            data-overlay-autofocus
            label={t("publish_destinations.editor.meta.status", "Status")}
            onChange={(event) =>
              onDestinationChange({
                enabled: event.currentTarget.value === "enabled",
              })
            }
            options={DESTINATION_STATUS_OPTIONS}
            value={destination.enabled ? "enabled" : "disabled"}
          />
        ) : null}

        <TextField
          data-overlay-autofocus={!showDestinationStatus}
          error={destinationErrors.itchAccountName}
          label={t(
            "publish_destinations.editor.itch.account_name",
            "Itch account name",
          )}
          onChange={(event) =>
            onDestinationChange({
              itchAccountName: event.currentTarget.value,
            })
          }
          placeholder="your-itch-username"
          value={destination.itchAccountName}
        />

        <TextField
          error={destinationErrors.itchGameSlug}
          label={t(
            "publish_destinations.editor.itch.game_slug",
            "Itch game slug",
          )}
          onChange={(event) =>
            onDestinationChange({
              itchGameSlug: event.currentTarget.value,
            })
          }
          placeholder="your-game-slug"
          value={destination.itchGameSlug}
        />
      </SurfacePanel>

      <SurfacePanel
        actions={
          onOpenCredentialComposer ? (
            <Button
              disabled={disabled}
              leadingIcon="plus"
              onClick={onOpenCredentialComposer}
              size="sm"
              variant="ghost"
            >
              {t(
                "publish_destinations.editor.credentials.new",
                "New credential",
              )}
            </Button>
          ) : null
        }
        className="project-detail-form-grid__span-full"
        description={
          showExpandedCopy
            ? t(
                "publish_destinations.editor.credentials.description",
                "Choose the stored publish credential that should back Butler uploads for this destination.",
              )
            : undefined
        }
        headerSeparated
        title={
          showExpandedCopy
            ? t(
                "publish_destinations.editor.credentials.title",
                "Credential state",
              )
            : t("publish_destinations.editor.credentials.access", "Access")
        }
        tone="inset"
      >
        <SelectField
          error={destinationErrors.credentialsId}
          label={t("publish_destinations.editor.meta.credential", "Credential")}
          onChange={(event) => {
            const nextCredentialId = event.currentTarget.value
              ? Number(event.currentTarget.value)
              : null;

            onDestinationChange({
              credentialsId: nextCredentialId,
              credentialsName: resolveCredentialNameById(
                credentials,
                nextCredentialId,
              ),
            });
          }}
          options={publishCredentialOptions}
          value={destination.credentialsId?.toString() ?? ""}
        />
      </SurfacePanel>

      <PublishDestinationBindingsSection
        buildTargets={buildTargets}
        description={
          showExpandedCopy
            ? t(
                "publish_destinations.editor.bindings.description",
                "Bind build targets to this destination and configure the per-target delivery fields each binding requires.",
              )
            : undefined
        }
        destination={destination}
        destinationErrors={destinationErrors}
        disabled={disabled}
        preferBindingCreateOverlay={preferBindingCreateOverlay}
        onAddBinding={onAddBinding}
        onBindingChange={onBindingChange}
        onPendingBindingTargetChange={onPendingBindingTargetChange}
        onRemoveBinding={onRemoveBinding}
        pendingBindingTargetId={pendingBindingTargetId}
        showItchUserversionTemplate={showItchUserversionTemplate}
        renderBindingFields={({
          binding,
          bindingErrors,
          onBindingChange: handleBindingFieldChange,
        }) => (
          <>
            <TextField
              error={bindingErrors.itchChannel}
              hint={
                showExpandedCopy
                  ? t(
                      "publish_destinations.editor.itch.channel_hint",
                      "Use the Itch channel that should receive this build target artifact.",
                    )
                  : undefined
              }
              label={t(
                "publish_destinations.editor.itch.channel",
                "Itch channel",
              )}
              onChange={(event) =>
                handleBindingFieldChange({
                  itchChannel: event.currentTarget.value,
                })
              }
              placeholder="windows"
              value={binding.itchChannel}
            />

            {showItchUserversionTemplate ? (
              <TextField
                hint={
                  showExpandedCopy
                    ? t(
                        "publish_destinations.editor.itch.userversion_hint",
                        "Optional template. Leave empty to use the git tag as the userversion.",
                      )
                    : undefined
                }
                label={t(
                  "publish_destinations.editor.itch.userversion_template",
                  "Itch userversion template",
                )}
                onChange={(event) =>
                  handleBindingFieldChange({
                    itchUserversionTemplate: event.currentTarget.value,
                  })
                }
                placeholder="{{git_tag}}"
                value={binding.itchUserversionTemplate}
              />
            ) : null}
          </>
        )}
        showBindingStatus={showDestinationStatus}
      />
    </>
  );
}

function PublishDestinationBindingsSection({
  autoFocusTargetSelector = false,
  buildTargets,
  description,
  destination,
  destinationErrors,
  disabled,
  preferBindingCreateOverlay = false,
  onAddBinding,
  onBindingChange,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
  renderBindingFields,
  showItchUserversionTemplate = true,
  showBindingStatus = true,
  title = "Target bindings",
}: PublishDestinationBindingsSectionProps) {
  const { t } = useLocalization();
  const { openOverlay } = useOverlay();
  const isOverlayBindingFlow = !showBindingStatus;
  const availableBindingTargets = buildTargets.filter(
    (target) =>
      !destination.bindings.some(
        (binding) => binding.buildTargetDraftId === target.id,
      ),
  );
  const pendingBindingTarget = availableBindingTargets.find(
    (target) => target.id === pendingBindingTargetId,
  );
  const hasAvailableBindingTargets = availableBindingTargets.length > 0;
  const shouldUseBindingSelectorOverlay =
    availableBindingTargets.length >= BINDING_SELECTOR_OVERLAY_THRESHOLD;

  const handleOpenBindingTargetSelector = async () => {
    if (disabled || availableBindingTargets.length === 0) {
      return;
    }

    const selectedTargetId = await openOverlay<string>(SelectListFullScreen, {
      description: t(
        "publish_destinations.editor.bindings.selector.description",
        "Search the unbound build targets and select one result to bind to this publish destination.",
      ),
      emptyStateCopy: t(
        "publish_destinations.editor.bindings.selector.empty_copy",
        "Every currently available build target is already bound to this destination.",
      ),
      emptyStateTitle: t(
        "publish_destinations.editor.bindings.selector.empty_title",
        "No unbound build targets are available.",
      ),
      items: buildBindingTargetItems(t, availableBindingTargets),
      title: t(
        "publish_destinations.editor.bindings.selector.title",
        "Select build target",
      ),
    });

    if (selectedTargetId) {
      onPendingBindingTargetChange(selectedTargetId);
    }
  };

  const handleOpenBindingCreateOverlay = async () => {
    if (disabled || availableBindingTargets.length === 0) {
      return;
    }

    const initialTarget =
      pendingBindingTarget ??
      availableBindingTargets.find(
        (target) => target.id === pendingBindingTargetId,
      ) ??
      availableBindingTargets[0];
    if (!initialTarget) {
      return;
    }

    const result =
      await openOverlay<PublishDestinationBindingEditorOverlayResult>(
        PublishDestinationBindingEditorOverlay,
        {
          buildTargets: availableBindingTargets,
          destinationKind: destination.kind,
          initialBinding:
            createEmptyPublishDestinationBindingDraft(initialTarget),
          initialTargetDraftId: initialTarget.id,
          mode: "create",
          showItchUserversionTemplate,
          showBindingStatus,
        },
      );

    if (!result) {
      return;
    }

    onAddBinding(result.targetDraftId, result.patch);
  };

  const handleOpenBindingEditOverlay = async (
    binding: PublishDestinationBindingDraft,
  ) => {
    if (disabled) {
      return;
    }

    const result =
      await openOverlay<PublishDestinationBindingEditorOverlayResult>(
        PublishDestinationBindingEditorOverlay,
        {
          buildTargets,
          destinationKind: destination.kind,
          initialBinding: binding,
          initialTargetDraftId: binding.buildTargetDraftId,
          mode: "edit",
          showItchUserversionTemplate,
          showBindingStatus,
        },
      );

    if (!result) {
      return;
    }

    onBindingChange(binding.id, result.patch);
  };

  if (isOverlayBindingFlow) {
    return (
      <SurfacePanel
        bodyClassName="wizard-form-grid"
        className="project-detail-form-grid__span-full"
        description={description}
        headerSeparated
        title={title}
        tone="inset"
      >
        <div className="project-detail-form-grid__span-full">
          <div className="project-detail-target-card">
            <div className="publish-destination-bindings-toolbar">
              {!hasAvailableBindingTargets ? (
                <div
                  aria-live="polite"
                  className="publish-destination-bindings-toolbar__empty-state"
                  role="status"
                >
                  <span className="ui-field__label">
                    {t("publish_destinations.editor.meta.target", "Target")}
                  </span>
                  <div className="publish-destination-bindings-toolbar__empty-value">
                    {t(
                      "publish_destinations.editor.bindings.none_available",
                      "No unbound build targets available.",
                    )}
                  </div>
                </div>
              ) : (
                <Button
                  className="publish-destination-bindings-toolbar__add"
                  data-overlay-autofocus={autoFocusTargetSelector}
                  disabled={disabled}
                  leadingIcon="plus"
                  onClick={() => {
                    void handleOpenBindingCreateOverlay();
                  }}
                  size="sm"
                  variant="secondary"
                >
                  {t(
                    "publish_destinations.editor.bindings.add_target",
                    "Add target",
                  )}
                </Button>
              )}
            </div>

            {destinationErrors.bindingsRoot ? (
              <p className="ui-field__error">
                {destinationErrors.bindingsRoot}
              </p>
            ) : null}
          </div>
        </div>

        <div className="project-detail-form-grid__span-full">
          {destination.bindings.length === 0 ? (
            <div className="feed-state">
              <p className="feed-state__title">
                {t(
                  "publish_destinations.editor.bindings.none_bound",
                  "No bound build targets.",
                )}
              </p>
            </div>
          ) : (
            <div className="project-detail-status-grid">
              {destination.bindings.map((binding) => {
                const bindingErrors =
                  destinationErrors.bindings[binding.id] ?? {};
                const buildTarget = resolveBindingBuildTarget(
                  binding,
                  buildTargets,
                );
                const bindingSummary =
                  destination.kind === "filesystem"
                    ? binding.filesystemDirectoryPath
                      ? t(
                          "publish_destinations.editor.bindings.summary.filesystem",
                          "Destination: {{path}}",
                          { path: binding.filesystemDirectoryPath },
                        )
                      : t(
                          "publish_destinations.editor.filesystem.directory_required",
                          "Destination directory is required.",
                        )
                    : t(
                        "publish_destinations.editor.bindings.summary.itch_channel",
                        "Channel: {{channel}}",
                        {
                          channel:
                            binding.itchChannel ||
                            t(
                              "publish_destinations.editor.meta.missing",
                              "Missing",
                            ),
                        },
                      );
                const firstBindingError =
                  bindingErrors.buildTarget ||
                  bindingErrors.filesystemDirectoryPath ||
                  bindingErrors.itchChannel ||
                  null;

                return (
                  <div className="project-detail-target-card" key={binding.id}>
                    <div className="publish-destination-binding-card__header">
                      <div className="publish-destination-binding-card__title-block">
                        <h4 className="project-detail-target-card__title">
                          {buildTarget?.name || binding.buildTargetName}
                        </h4>
                      </div>

                      <div className="publish-destination-binding-card__actions">
                        <Button
                          disabled={disabled}
                          onClick={() => {
                            void handleOpenBindingEditOverlay(binding);
                          }}
                          size="sm"
                          variant="ghost"
                        >
                          {t(
                            "publish_destinations.editor.actions.edit",
                            "Edit",
                          )}
                        </Button>
                        <IconButton
                          className="publish-destination-binding-card__remove"
                          disabled={disabled}
                          icon="trash"
                          label={t(
                            "publish_destinations.editor.bindings.remove_binding",
                            "Remove binding for {{name}}",
                            {
                              name:
                                buildTarget?.name || binding.buildTargetName,
                            },
                          )}
                          onClick={() => onRemoveBinding(binding.id)}
                          size="sm"
                          variant="ghost"
                        />
                      </div>
                    </div>

                    <div className="publish-destination-binding-card__summary">
                      <p>{bindingSummary}</p>
                      {destination.kind === "itch" &&
                      binding.itchUserversionTemplate ? (
                        <p className="publish-destination-binding-card__meta">
                          {t(
                            "publish_destinations.editor.bindings.summary.userversion",
                            "Userversion: {{template}}",
                            {
                              template: binding.itchUserversionTemplate,
                            },
                          )}
                        </p>
                      ) : null}
                      {firstBindingError ? (
                        <p className="ui-field__error">{firstBindingError}</p>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </SurfacePanel>
    );
  }

  return (
    <SurfacePanel
      bodyClassName="wizard-form-grid"
      className="project-detail-form-grid__span-full"
      description={description}
      headerSeparated
      title={title}
      tone="inset"
    >
      <div className="project-detail-form-grid__span-full">
        <div className="project-detail-target-card">
          <div className="publish-destination-bindings-toolbar">
            {!hasAvailableBindingTargets ? (
              <div
                aria-live="polite"
                className="publish-destination-bindings-toolbar__empty-state"
                role="status"
              >
                <span className="ui-field__label">
                  {t("publish_destinations.editor.meta.target", "Target")}
                </span>
                <div className="publish-destination-bindings-toolbar__empty-value">
                  {t(
                    "publish_destinations.editor.bindings.none_available",
                    "No unbound build targets available.",
                  )}
                </div>
              </div>
            ) : shouldUseBindingSelectorOverlay ? (
              <Button
                className="publish-destination-bindings-toolbar__selector-button"
                data-overlay-autofocus={autoFocusTargetSelector}
                disabled={disabled}
                leadingIcon="search"
                onClick={() => {
                  void handleOpenBindingTargetSelector();
                }}
                size="sm"
                variant="ghost"
              >
                {pendingBindingTarget
                  ? t(
                      "publish_destinations.editor.bindings.selector.pending_target",
                      "Target: {{name}}",
                      { name: pendingBindingTarget.name },
                    )
                  : t(
                      "publish_destinations.editor.bindings.selector.select",
                      "Select target",
                    )}
              </Button>
            ) : (
              <SelectField
                className="publish-destination-bindings-toolbar__selector"
                data-overlay-autofocus={autoFocusTargetSelector}
                disabled={disabled}
                label={t("publish_destinations.editor.meta.target", "Target")}
                onChange={(event) =>
                  onPendingBindingTargetChange(event.currentTarget.value)
                }
                options={buildBindingTargetOptions(availableBindingTargets)}
                value={pendingBindingTargetId}
              />
            )}
            {hasAvailableBindingTargets ? (
              <Button
                className="publish-destination-bindings-toolbar__add"
                disabled={disabled || !pendingBindingTargetId}
                leadingIcon="plus"
                onClick={() => {
                  if (preferBindingCreateOverlay) {
                    void handleOpenBindingCreateOverlay();
                    return;
                  }

                  onAddBinding();
                }}
                size="sm"
                variant="secondary"
              >
                {t(
                  "publish_destinations.editor.bindings.add_target",
                  "Add target",
                )}
              </Button>
            ) : null}
          </div>

          {destinationErrors.bindingsRoot ? (
            <p className="ui-field__error">{destinationErrors.bindingsRoot}</p>
          ) : null}
        </div>
      </div>

      <div className="project-detail-form-grid__span-full">
        {destination.bindings.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">
              {t(
                "publish_destinations.editor.bindings.none_bound",
                "No bound build targets.",
              )}
            </p>
          </div>
        ) : (
          <div className="project-detail-status-grid">
            {destination.bindings.map((binding) => {
              const bindingErrors =
                destinationErrors.bindings[binding.id] ?? {};
              const buildTarget = resolveBindingBuildTarget(
                binding,
                buildTargets,
              );

              return (
                <div className="project-detail-target-card" key={binding.id}>
                  <div className="publish-destination-binding-card__header">
                    <div className="publish-destination-binding-card__title-block">
                      <h4 className="project-detail-target-card__title">
                        {buildTarget?.name || binding.buildTargetName}
                      </h4>
                    </div>

                    <IconButton
                      className="publish-destination-binding-card__remove"
                      disabled={disabled}
                      icon="trash"
                      label={t(
                        "publish_destinations.editor.bindings.remove_binding",
                        "Remove binding for {{name}}",
                        { name: buildTarget?.name || binding.buildTargetName },
                      )}
                      onClick={() => onRemoveBinding(binding.id)}
                      size="sm"
                      variant="ghost"
                    />
                  </div>

                  <div className="wizard-form-grid">
                    {showBindingStatus ? (
                      <SelectField
                        label={t(
                          "publish_destinations.editor.meta.status",
                          "Status",
                        )}
                        onChange={(event) =>
                          onBindingChange(binding.id, {
                            enabled: event.currentTarget.value === "enabled",
                          })
                        }
                        options={BINDING_STATUS_OPTIONS}
                        value={binding.enabled ? "enabled" : "disabled"}
                      />
                    ) : null}

                    {renderBindingFields({
                      binding,
                      bindingErrors,
                      disabled,
                      onBindingChange: (patch) =>
                        onBindingChange(binding.id, patch),
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </SurfacePanel>
  );
}

export function createEmptyPublishDestinationDraft(
  kind: PublishDestinationKind = "filesystem",
): PublishDestinationDraft {
  return {
    id: createDraftId("destination"),
    publishTargetId: null,
    name: derivePublishDestinationName(kind),
    kind,
    enabled: true,
    itchAccountName: "",
    itchGameSlug: "",
    credentialsId: null,
    credentialsName: null,
    bindings: [],
  };
}

export function buildPublishDestinationDrafts(
  publishTargets: RepositoryPublishTargetInspection[],
  buildTargets: ProjectBuildTargetReference[],
): PublishDestinationDraft[] {
  return publishTargets.map((publishTarget) => {
    const config = parseJsonObject(publishTarget.config_json);
    const kind: PublishDestinationKind =
      publishTarget.kind === "itch" ? "itch" : "filesystem";

    return {
      id: createDraftId(`destination-${publishTarget.publish_target_id}`),
      publishTargetId: publishTarget.publish_target_id,
      name: derivePublishDestinationName(kind),
      kind,
      enabled: publishTarget.enabled,
      itchAccountName: readJsonStringField(config, "account_name") || "",
      itchGameSlug: readJsonStringField(config, "game_slug") || "",
      credentialsId: publishTarget.credentials?.credential_id ?? null,
      credentialsName: publishTarget.credentials?.name ?? null,
      bindings: publishTarget.bindings.map((binding) => {
        const target = buildTargets.find(
          (entry) => entry.buildTargetId === binding.build_target_id,
        );
        const options = parseJsonObject(binding.options_json);

        return {
          id: createDraftId(
            `binding-${publishTarget.publish_target_id}-${binding.build_target_id}`,
          ),
          buildTargetDraftId:
            target?.id ||
            createDraftId(`missing-target-${binding.build_target_id}`),
          buildTargetId: binding.build_target_id,
          buildTargetName: binding.build_target_name,
          enabled: binding.enabled,
          filesystemDirectoryPath:
            readJsonStringField(options, "directory_path") || "",
          itchChannel: readJsonStringField(options, "channel") || "",
          itchUserversionTemplate:
            readJsonStringField(options, "userversion_template") || "",
        };
      }),
    };
  });
}

export function createEmptyPublishDestinationErrors(): PublishDestinationDraftErrors {
  return {
    bindings: {},
  };
}

export function createEmptyPublishDestinationValidationErrors(): PublishDestinationValidationErrors {
  return {
    destinations: {},
  };
}

export function hasPublishDestinationValidationErrors(
  errors: PublishDestinationValidationErrors,
) {
  return Boolean(
    errors.root ||
    Object.values(errors.destinations).some((destination) =>
      hasPublishDestinationDraftErrors(destination),
    ),
  );
}

export function validatePublishDestinationDrafts(
  destinations: PublishDestinationDraft[],
  buildTargets: ProjectBuildTargetReference[],
): PublishDestinationValidationErrors {
  const errors = createEmptyPublishDestinationValidationErrors();
  const destinationKinds = new Set<PublishDestinationKind>();
  const duplicateDestinationKinds = new Set<PublishDestinationKind>();
  const consumingBindingsByTarget = new Map<string, string[]>();

  for (const destination of destinations) {
    const destinationErrors = createEmptyPublishDestinationErrors();
    if (destinationKinds.has(destination.kind)) {
      duplicateDestinationKinds.add(destination.kind);
    } else {
      destinationKinds.add(destination.kind);
    }

    if (destination.kind === "itch") {
      if (!destination.itchAccountName.trim()) {
        destinationErrors.itchAccountName = "Itch account name is required.";
      }
      if (!destination.itchGameSlug.trim()) {
        destinationErrors.itchGameSlug = "Itch game slug is required.";
      }
    }

    if (destination.bindings.length === 0) {
      destinationErrors.bindingsRoot =
        "No build targets are currently bound to this destination.";
    }

    for (const binding of destination.bindings) {
      const bindingErrors: PublishDestinationBindingErrors = {};
      const buildTarget = resolveBindingBuildTarget(binding, buildTargets);

      if (!buildTarget) {
        bindingErrors.buildTarget =
          "This binding points at a build target that is no longer active.";
      }

      if (destination.kind === "filesystem") {
        if (!binding.filesystemDirectoryPath.trim()) {
          bindingErrors.filesystemDirectoryPath =
            "Destination directory is required.";
        } else if (!looksLikeAbsolutePath(binding.filesystemDirectoryPath)) {
          bindingErrors.filesystemDirectoryPath =
            "Destination directory must be an absolute path.";
        }
      } else if (!binding.itchChannel.trim()) {
        bindingErrors.itchChannel = "Itch channel is required.";
      }

      if (binding.enabled && buildTarget && destination.kind === "filesystem") {
        const current = consumingBindingsByTarget.get(buildTarget.id) ?? [];
        current.push(formatPublishDestinationTitle(destination));
        consumingBindingsByTarget.set(buildTarget.id, current);
      }

      destinationErrors.bindings[binding.id] = bindingErrors;
    }

    errors.destinations[destination.id] = destinationErrors;
  }

  const conflictingTargets = Array.from(
    consumingBindingsByTarget.entries(),
  ).filter(
    ([, destinationNamesForTarget]) => destinationNamesForTarget.length > 1,
  );

  const rootErrors: string[] = [];

  if (duplicateDestinationKinds.size > 0) {
    rootErrors.push(
      Array.from(duplicateDestinationKinds)
        .map(
          (kind) =>
            `${formatPublishDestinationKindLabel(kind)} is already added; only one destination of each type is allowed.`,
        )
        .join(" | "),
    );
  }

  if (conflictingTargets.length > 0) {
    rootErrors.push(
      conflictingTargets
        .map(([buildTargetDraftId, destinationNamesForTarget]) => {
          const buildTarget = buildTargets.find(
            (target) => target.id === buildTargetDraftId,
          );
          return `${buildTarget?.name || "Unknown target"}: ${destinationNamesForTarget.join(", ")}`;
        })
        .join(" | "),
    );

    for (const [buildTargetDraftId] of conflictingTargets) {
      for (const destination of destinations) {
        for (const binding of destination.bindings) {
          if (binding.buildTargetDraftId !== buildTargetDraftId) {
            continue;
          }
          if (destination.kind !== "filesystem" || !binding.enabled) {
            continue;
          }

          const destinationErrors =
            errors.destinations[destination.id] ??
            createEmptyPublishDestinationErrors();
          const bindingErrors = destinationErrors.bindings[binding.id] ?? {};
          bindingErrors.buildTarget =
            "Only one enabled consuming binding is allowed per build target.";
          destinationErrors.bindings[binding.id] = bindingErrors;
          errors.destinations[destination.id] = destinationErrors;
        }
      }
    }
  }

  if (rootErrors.length > 0) {
    errors.root = rootErrors.join(" | ");
  }

  return errors;
}

export function buildCreateProjectPublishTargetsInput(
  destinations: PublishDestinationDraft[],
  buildTargets: ProjectBuildTargetReference[],
): CreateRepositoryProjectPublishTargetInput[] {
  return destinations.map((destination) => ({
    name: derivePublishDestinationName(destination.kind),
    kind: destination.kind,
    enabled: destination.enabled,
    config_json: JSON.stringify(buildPublishTargetConfig(destination)),
    credentials_id:
      destination.kind === "itch" ? destination.credentialsId : null,
    bindings: destination.bindings.map((binding) => {
      const buildTarget = resolveBindingBuildTarget(binding, buildTargets);
      return {
        build_target_name: (
          buildTarget?.name || binding.buildTargetName
        ).trim(),
        enabled: binding.enabled,
        options_json: JSON.stringify(
          buildPublishBindingOptions(destination.kind, binding),
        ),
      };
    }),
  }));
}

export function buildUpdateProjectPublishTargetsInput(
  destinations: PublishDestinationDraft[],
  buildTargets: ProjectBuildTargetReference[],
): UpdateRepositoryProjectPublishTargetInput[] {
  return destinations.map((destination) => ({
    publish_target_id: destination.publishTargetId,
    name: derivePublishDestinationName(destination.kind),
    kind: destination.kind,
    enabled: destination.enabled,
    config_json: JSON.stringify(buildPublishTargetConfig(destination)),
    credentials_id:
      destination.kind === "itch" ? destination.credentialsId : null,
    bindings: destination.bindings.map((binding) => {
      const buildTarget = resolveBindingBuildTarget(binding, buildTargets);
      return {
        build_target_id: buildTarget?.buildTargetId ?? binding.buildTargetId,
        build_target_name: (
          buildTarget?.name || binding.buildTargetName
        ).trim(),
        enabled: binding.enabled,
        options_json: JSON.stringify(
          buildPublishBindingOptions(destination.kind, binding),
        ),
      };
    }),
  }));
}

export function collectBuildTargetBindingImpact(
  destinations: PublishDestinationDraft[],
  buildTargetDraftId: string,
): string[] {
  return destinations
    .filter((destination) =>
      destination.bindings.some(
        (binding) => binding.buildTargetDraftId === buildTargetDraftId,
      ),
    )
    .map((destination) => formatPublishDestinationTitle(destination))
    .sort((left, right) => left.localeCompare(right));
}

export function removeBuildTargetBindings(
  destinations: PublishDestinationDraft[],
  buildTargetDraftId: string,
): PublishDestinationDraft[] {
  return destinations.map((destination) => ({
    ...destination,
    bindings: destination.bindings.filter(
      (binding) => binding.buildTargetDraftId !== buildTargetDraftId,
    ),
  }));
}

export function listUnboundBuildTargetNames(
  destinations: PublishDestinationDraft[],
  buildTargets: ProjectBuildTargetReference[],
): string[] {
  const boundTargets = new Set(
    destinations.flatMap((destination) =>
      destination.bindings.map((binding) => binding.buildTargetDraftId),
    ),
  );

  return buildTargets
    .filter((target) => !boundTargets.has(target.id))
    .map((target) => target.name)
    .sort((left, right) => left.localeCompare(right));
}

export function buildPublishDestinationReviewSummary(
  destinations: PublishDestinationDraft[],
  buildTargets: ProjectBuildTargetReference[],
): PublishDestinationReviewSummary[] {
  return destinations.map((destination) => {
    const destinationLabel = formatPublishDestinationTitle(destination);

    return {
      id: destination.id,
      name: destinationLabel,
      kindLabel: formatPublishDestinationKindLabel(destination.kind),
      enabled: destination.enabled,
      bindingTargetNames: collectPublishDestinationBindingTargets(
        destination,
        buildTargets,
      ),
      missingCredential:
        destination.kind === "itch" && destination.credentialsId === null,
    };
  });
}

function resolvePendingBindingTargetId(
  destination: PublishDestinationDraft,
  buildTargets: ProjectBuildTargetReference[],
  pendingBindingTargetId?: string,
): string {
  if (pendingBindingTargetId) {
    return pendingBindingTargetId;
  }

  const availableBindingTarget = buildTargets.find(
    (target) =>
      !destination.bindings.some(
        (binding) => binding.buildTargetDraftId === target.id,
      ),
  );

  return availableBindingTarget?.id ?? "";
}

function buildBindingTargetOptions(
  tOrBuildTargets: Translate | ProjectBuildTargetReference[],
  maybeBuildTargets?: ProjectBuildTargetReference[],
): SelectOption[] {
  const buildTargets = Array.isArray(tOrBuildTargets)
    ? tOrBuildTargets
    : (maybeBuildTargets ?? []);
  const translate = Array.isArray(tOrBuildTargets) ? null : tOrBuildTargets;

  return [
    {
      disabled: buildTargets.length === 0,
      label:
        buildTargets.length === 0
          ? (translate?.(
              "publish_destinations.editor.bindings.selector.none_available",
              "No unbound build targets available",
            ) ?? "No unbound build targets available")
          : (translate?.(
              "publish_destinations.editor.bindings.selector.select_build_target",
              "Select a build target",
            ) ?? "Select a build target"),
      value: "",
    },
    ...buildTargets.map((target) => ({
      label: target.name,
      value: target.id,
    })),
  ];
}

function buildBindingTargetItems(
  tOrBuildTargets: Translate | ProjectBuildTargetReference[],
  maybeBuildTargets?: ProjectBuildTargetReference[],
): SelectListItem[] {
  const buildTargets = Array.isArray(tOrBuildTargets)
    ? tOrBuildTargets
    : (maybeBuildTargets ?? []);
  const translate = Array.isArray(tOrBuildTargets) ? null : tOrBuildTargets;

  return buildTargets.map((target) => ({
    id: target.id,
    label: target.name,
    subtitle: target.buildTargetId
      ? (translate?.(
          "publish_destinations.editor.bindings.target_id",
          "Build target id {{id}}",
          { id: target.buildTargetId },
        ) ?? `Build target id ${target.buildTargetId}`)
      : (translate?.(
          "publish_destinations.editor.bindings.draft_only",
          "Draft-only build target",
        ) ?? "Draft-only build target"),
  }));
}

function buildPublishCredentialOptions(
  tOrCredentials: Translate | SecretCredentialSetting[],
  maybeCredentialsOrSelected: SecretCredentialSetting[] | number | null,
  maybeSelectedCredentialIdOrName?: number | string | null,
  maybeSelectedCredentialName?: string | null,
): SelectOption[] {
  const credentials = Array.isArray(tOrCredentials)
    ? tOrCredentials
    : ((maybeCredentialsOrSelected as SecretCredentialSetting[] | undefined) ??
      []);
  const selectedCredentialId = Array.isArray(tOrCredentials)
    ? (maybeCredentialsOrSelected as number | null)
    : typeof maybeSelectedCredentialIdOrName === "number"
      ? maybeSelectedCredentialIdOrName
      : null;
  const selectedCredentialName = Array.isArray(tOrCredentials)
    ? typeof maybeSelectedCredentialIdOrName === "string"
      ? maybeSelectedCredentialIdOrName
      : null
    : (maybeSelectedCredentialName ?? null);
  const translate = Array.isArray(tOrCredentials) ? null : tOrCredentials;

  const selectableCredentials = credentials.filter(isItchCredentialSelectable);
  const options: SelectOption[] = [
    {
      label:
        selectableCredentials.length === 0
          ? (translate?.(
              "publish_destinations.editor.credentials.none_available",
              "No stored Itch credentials available",
            ) ?? "No stored Itch credentials available")
          : (translate?.(
              "publish_destinations.editor.credentials.none_selected",
              "No Itch credential selected",
            ) ?? "No Itch credential selected"),
      value: "",
    },
    ...selectableCredentials.map((credential) => ({
      label: credential.name,
      value: credential.credential_id.toString(),
    })),
  ];

  if (
    selectedCredentialId !== null &&
    !selectableCredentials.some(
      (credential) => credential.credential_id === selectedCredentialId,
    )
  ) {
    const selectedCredential = credentials.find(
      (credential) => credential.credential_id === selectedCredentialId,
    );

    options.push({
      label:
        selectedCredential?.name ||
        selectedCredentialName?.trim() ||
        translate?.(
          "publish_destinations.editor.credentials.current",
          "Current credential #{{id}}",
          { id: selectedCredentialId },
        ) ||
        `Current credential #${selectedCredentialId}`,
      value: selectedCredentialId.toString(),
    });
  }

  return options;
}

function buildPublishTargetConfig(destination: PublishDestinationDraft) {
  if (destination.kind === "filesystem") {
    return {};
  }

  const config: Record<string, string> = {
    account_name: destination.itchAccountName.trim(),
    game_slug: destination.itchGameSlug.trim(),
  };
  return config;
}

function buildPublishBindingOptions(
  kind: PublishDestinationKind,
  binding: PublishDestinationBindingDraft,
) {
  if (kind === "filesystem") {
    return {
      operation: "move",
      directory_path: binding.filesystemDirectoryPath.trim(),
    };
  }

  const options: Record<string, string> = {
    channel: binding.itchChannel.trim(),
  };
  if (binding.itchUserversionTemplate.trim()) {
    options.userversion_template = binding.itchUserversionTemplate.trim();
  }
  return options;
}

function resolveBindingBuildTarget(
  binding: PublishDestinationBindingDraft,
  buildTargets: ProjectBuildTargetReference[],
) {
  return (
    buildTargets.find((target) => target.id === binding.buildTargetDraftId) ??
    null
  );
}

function collectPublishDestinationBindingTargets(
  destination: PublishDestinationDraft,
  buildTargets: ProjectBuildTargetReference[],
) {
  return destination.bindings
    .map((binding) => {
      const buildTarget = resolveBindingBuildTarget(binding, buildTargets);
      return buildTarget?.name || binding.buildTargetName || "Unknown target";
    })
    .sort((left, right) => left.localeCompare(right));
}

function createEmptyPublishDestinationBindingDraft(
  buildTarget: ProjectBuildTargetReference,
): PublishDestinationBindingDraft {
  return {
    id: createDraftId("binding"),
    buildTargetDraftId: buildTarget.id,
    buildTargetId: buildTarget.buildTargetId,
    buildTargetName: buildTarget.name,
    enabled: true,
    filesystemDirectoryPath: "",
    itchChannel: "",
    itchUserversionTemplate: "",
  };
}

function createDraftId(prefix: string) {
  return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
}

function formatPublishDestinationKindLabel(
  tOrKind: Translate | PublishDestinationKind,
  maybeKind?: PublishDestinationKind,
) {
  const kind =
    typeof tOrKind === "string" ? tOrKind : (maybeKind ?? "filesystem");
  const translate = typeof tOrKind === "string" ? null : tOrKind;

  return kind === "filesystem"
    ? (translate?.("publish_destinations.editor.kind.folder", "Folder") ??
        "Folder")
    : (translate?.("publish_destinations.editor.kind.itch", "Itch") ?? "Itch");
}

function formatPublishDestinationTitle(
  tOrDestination: Translate | PublishDestinationDraft,
  maybeDestination?: PublishDestinationDraft,
) {
  const destination =
    typeof tOrDestination === "object" ? tOrDestination : maybeDestination;

  if (!destination) {
    return "";
  }

  return derivePublishDestinationName(destination.kind);
}

function formatPublishDestinationBindingCount(
  tOrCount: Translate | number,
  maybeCount?: number,
) {
  const count = typeof tOrCount === "number" ? tOrCount : (maybeCount ?? 0);
  const translate = typeof tOrCount === "number" ? null : tOrCount;

  return count === 1
    ? (translate?.(
        "publish_destinations.editor.bindings.one_target",
        "1 target",
      ) ?? "1 target")
    : (translate?.(
        "publish_destinations.editor.bindings.many_targets",
        "{{count}} targets",
        { count },
      ) ?? `${count} targets`);
}

function formatPublishDestinationBindingPreview(
  tOrNames: Translate | string[],
  maybeBindingTargetNames?: string[],
) {
  const bindingTargetNames = Array.isArray(tOrNames)
    ? tOrNames
    : (maybeBindingTargetNames ?? []);
  const translate = Array.isArray(tOrNames) ? null : tOrNames;

  if (bindingTargetNames.length === 0) {
    return (
      translate?.(
        "publish_destinations.editor.bindings.none_bound",
        "No bound targets",
      ) ?? "No bound targets"
    );
  }

  if (bindingTargetNames.length <= 2) {
    return bindingTargetNames.join(", ");
  }

  return (
    translate?.(
      "publish_destinations.editor.bindings.more_targets",
      "{{preview}} +{{count}} more",
      {
        count: bindingTargetNames.length - 2,
        preview: bindingTargetNames.slice(0, 2).join(", "),
      },
    ) ??
    `${bindingTargetNames.slice(0, 2).join(", ")} +${bindingTargetNames.length - 2} more`
  );
}

function formatPublishDestinationOperationalSummary(
  tOrDestination: Translate | PublishDestinationDraft,
  maybeDestination?: PublishDestinationDraft,
) {
  const destination =
    typeof tOrDestination === "object" ? tOrDestination : maybeDestination;
  const translate = typeof tOrDestination === "object" ? null : tOrDestination;

  if (!destination) {
    return "";
  }

  if (destination.kind === "filesystem") {
    return (
      translate?.(
        "publish_destinations.editor.summary.move_artifacts",
        "Move artifacts",
      ) ?? "Move artifacts"
    );
  }

  return destination.credentialsId === null
    ? (translate?.("publish_destinations.editor.summary.missing", "Missing") ??
        "Missing")
    : (translate?.(
        "publish_destinations.editor.summary.configured",
        "Configured",
      ) ?? "Configured");
}

function buildPublishDestinationQuickViewCopy(
  tOrDestination: Translate | PublishDestinationDraft,
  maybeDestinationOrBindingTargets: PublishDestinationDraft | string[],
  maybeBindingTargetNames?: string[],
) {
  const destination =
    typeof tOrDestination === "object"
      ? tOrDestination
      : (maybeDestinationOrBindingTargets as PublishDestinationDraft);
  const bindingTargetNames = Array.isArray(maybeDestinationOrBindingTargets)
    ? maybeDestinationOrBindingTargets
    : (maybeBindingTargetNames ?? []);
  const translate = typeof tOrDestination === "object" ? null : tOrDestination;

  if (!destination) {
    return "";
  }

  if (destination.kind === "filesystem") {
    return bindingTargetNames.length === 0
      ? (translate?.(
          "publish_destinations.editor.quick_view.filesystem.empty",
          "No build targets are currently bound to this folder destination.",
        ) ?? "No build targets are currently bound to this folder destination.")
      : (translate?.(
          "publish_destinations.editor.quick_view.filesystem.bound",
          "Artifacts will move into the configured folder for each bound build target.",
        ) ??
          "Artifacts will move into the configured folder for each bound build target.");
  }

  return destination.credentialsId === null
    ? (translate?.(
        "publish_destinations.editor.quick_view.itch.missing",
        "Select a publish credential before this Itch destination can upload bound targets.",
      ) ??
        "Select a publish credential before this Itch destination can upload bound targets.")
    : (translate?.(
        "publish_destinations.editor.quick_view.itch.configured",
        "Bound targets will upload to Itch with the selected credential and per-target channels.",
      ) ??
        "Bound targets will upload to Itch with the selected credential and per-target channels.");
}

function formatPublishDestinationErrorPreview(
  errors: PublishDestinationDraftErrors,
) {
  const messages = collectPublishDestinationErrorMessages(errors);

  if (messages.length === 0) {
    return null;
  }

  if (messages.length === 1) {
    return messages[0];
  }

  return `${messages[0]} +${messages.length - 1} more`;
}

function collectPublishDestinationErrorMessages(
  errors: PublishDestinationDraftErrors,
) {
  const messages: string[] = [];

  for (const candidate of [
    errors.credentialsId,
    errors.itchAccountName,
    errors.itchGameSlug,
    errors.bindingsRoot,
  ]) {
    if (candidate) {
      messages.push(candidate);
    }
  }

  for (const bindingErrors of Object.values(errors.bindings)) {
    for (const candidate of [
      bindingErrors.buildTarget,
      bindingErrors.filesystemDirectoryPath,
    ]) {
      if (candidate) {
        messages.push(candidate);
      }
    }
  }

  return Array.from(new Set(messages));
}

function derivePublishDestinationName(kind: PublishDestinationKind) {
  return formatPublishDestinationKindLabel(kind);
}

function listPublishDestinationKinds(): PublishDestinationKind[] {
  return ["filesystem", "itch"];
}

function clonePublishDestinationDraft(destination: PublishDestinationDraft) {
  return {
    ...destination,
    bindings: destination.bindings.map((binding) => ({ ...binding })),
  };
}

function buildPublishCredentialComposerErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not save the credential.";
}

function getPublishDestinationAdapter(
  kind: PublishDestinationKind,
): PublishDestinationAdapterDefinition {
  if (kind === "filesystem") {
    return {
      Component: FilesystemPublishDestinationAdapter,
      kind,
      label: formatPublishDestinationKindLabel(kind),
    };
  }

  return {
    Component: ItchPublishDestinationAdapter,
    kind,
    label: formatPublishDestinationKindLabel(kind),
  };
}

function hasPublishDestinationDraftErrors(
  errors: PublishDestinationDraftErrors,
) {
  return Boolean(
    errors.credentialsId ||
    errors.itchAccountName ||
    errors.itchGameSlug ||
    errors.bindingsRoot ||
    Object.values(errors.bindings).some((bindingErrors) =>
      hasPublishDestinationBindingErrors(bindingErrors),
    ),
  );
}

function hasPublishDestinationBindingErrors(
  errors: PublishDestinationBindingErrors,
) {
  return Boolean(errors.buildTarget || errors.filesystemDirectoryPath);
}

function resolveCredentialNameById(
  credentials: SecretCredentialSetting[],
  credentialId: number | null,
) {
  if (credentialId === null) {
    return null;
  }

  const matchedCredential = credentials.find(
    (credential) => credential.credential_id === credentialId,
  );

  return matchedCredential?.name ?? null;
}

function isItchCredentialSelectable(credential: SecretCredentialSetting) {
  return (
    credential.kind === "itch-api-key" &&
    credential.config_summary.status === "ready"
  );
}

function parseJsonObject(value: string): Record<string, unknown> | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return null;
    }

    return parsed as Record<string, unknown>;
  } catch {
    return null;
  }
}

function readJsonStringField(
  value: Record<string, unknown> | null,
  key: string,
) {
  const candidate = value?.[key];

  if (typeof candidate !== "string") {
    return null;
  }

  const trimmed = candidate.trim();
  return trimmed ? trimmed : null;
}

function looksLikeAbsolutePath(value: string) {
  return (
    /^[a-zA-Z]:[\\/]/.test(value) ||
    value.startsWith("/") ||
    value.startsWith("\\\\")
  );
}
