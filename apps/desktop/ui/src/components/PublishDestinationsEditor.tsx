import { useState, type ReactElement } from "react";

import { Button, IconButton } from "./Button";
import { SelectField, TextField, type SelectOption } from "./Field";
import { useOverlay } from "./OverlayManager";
import { PathPickerField } from "./PathPickerField";
import SelectListFullScreen, {
  type SelectListItem,
} from "./SelectListFullScreen";
import { Badge, SurfacePanel } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import CredentialComposerModal from "./forms/CredentialComposerModal";
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
  itchButlerPath: string;
  credentialsId: number | null;
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
  itchButlerPath?: string;
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
  usesHostTransportProbe: boolean;
};

type PublishDestinationsEditorProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  destinations: PublishDestinationDraft[];
  disabled?: boolean;
  errors?: PublishDestinationValidationErrors;
  onChange: (next: PublishDestinationDraft[]) => void;
  onSaveCredential?: (
    destinationId: string,
    input: SaveSecretCredentialInput,
  ) => Promise<void> | void;
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

export function PublishDestinationsEditor({
  buildTargets,
  credentials,
  destinations,
  disabled = false,
  errors,
  onChange,
  onSaveCredential,
}: PublishDestinationsEditorProps) {
  const { openOverlay } = useOverlay();
  const [showDestinationMenu, setShowDestinationMenu] = useState(false);
  const [pendingDestinationRemovalId, setPendingDestinationRemovalId] =
    useState<string | null>(null);
  const [pendingBindingTargetSelections, setPendingBindingTargetSelections] =
    useState<Record<string, string>>({});

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

    await openOverlay<SaveSecretCredentialInput>(CredentialComposerModal, {
      onSubmit: (input: SaveSecretCredentialInput) =>
        onSaveCredential(destinationId, input),
      providerLabel: "Itch.io",
      scope: "publish",
    });
  };

  const handleAddBinding = (destinationId: string) => {
    const destination = destinations.find(
      (entry) => entry.id === destinationId,
    );
    if (!destination) {
      return;
    }

    const selectedTargetDraftId = resolvePendingBindingTargetId(
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
                createEmptyPublishDestinationBindingDraft(selectedTarget),
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
        <div className="publish-destination-menu">
          <Button
            disabled={disabled}
            leadingIcon="plus"
            onClick={() => setShowDestinationMenu((current) => !current)}
            size="sm"
            variant="secondary"
          >
            Add destination
          </Button>
          {showDestinationMenu ? (
            <div
              aria-label="Destination list"
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
                    {formatPublishDestinationKindLabel(kind)}
                  </button>
                );
              })}
            </div>
          ) : null}
        </div>
      </div>

      {errors?.root ? (
        <p className="feed-banner feed-banner--error">{errors.root}</p>
      ) : null}

      {pendingDestinationRemoval ? (
        <div className="wizard-callout wizard-callout--compact wizard-callout--auth">
          <div className="wizard-callout__header">
            <div>
              <p className="wizard-callout__title">
                Confirm destination removal
              </p>
              <p className="wizard-callout__copy">
                Removing{" "}
                {formatPublishDestinationTitle(pendingDestinationRemoval)} also
                removes persisted bindings for{" "}
                {pendingRemovalTargets.join(", ") || "its bound build targets"}.
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
              Remove destination
            </Button>
            <Button
              disabled={disabled}
              onClick={() => setPendingDestinationRemovalId(null)}
              size="sm"
              variant="ghost"
            >
              Cancel
            </Button>
          </div>
        </div>
      ) : null}

      {destinations.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">
            No publish destinations configured.
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
        const adapter = getPublishDestinationAdapter(destination.kind);
        const AdapterComponent = adapter.Component;

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
                    label={`Remove ${adapter.label} destination`}
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
                    {destination.enabled ? "enabled" : "disabled"}
                  </Badge>
                </div>
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
            />
          </VerticalAccordion>
        );
      })}
    </div>
  );
}

type PublishDestinationAdapterComponentProps = {
  buildTargets: ProjectBuildTargetReference[];
  credentials: SecretCredentialSetting[];
  destination: PublishDestinationDraft;
  destinationErrors: PublishDestinationDraftErrors;
  disabled: boolean;
  onAddBinding: () => void;
  onBindingChange: (
    bindingId: string,
    patch: Partial<PublishDestinationBindingDraft>,
  ) => void;
  onDestinationChange: (patch: Partial<PublishDestinationDraft>) => void;
  onOpenCredentialComposer?: () => void;
  onPendingBindingTargetChange: (nextTargetId: string) => void;
  onRemoveBinding: (bindingId: string) => void;
  pendingBindingTargetId: string;
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
  buildTargets: ProjectBuildTargetReference[];
  destination: PublishDestinationDraft;
  destinationErrors: PublishDestinationDraftErrors;
  disabled: boolean;
  onAddBinding: () => void;
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
};

function FilesystemPublishDestinationAdapter({
  buildTargets,
  destination,
  destinationErrors,
  disabled,
  onAddBinding,
  onBindingChange,
  onDestinationChange,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
}: PublishDestinationAdapterComponentProps) {
  return (
    <>
      <SurfacePanel
        bodyClassName="wizard-form-grid"
        className="project-detail-form-grid__span-full"
        description="Configure the lifecycle state for this filesystem publish destination. Bound target paths stay with each individual binding."
        headerSeparated
        title="Destination identity"
        tone="inset"
      >
        <SelectField
          label="Status"
          onChange={(event) =>
            onDestinationChange({
              enabled: event.currentTarget.value === "enabled",
            })
          }
          options={DESTINATION_STATUS_OPTIONS}
          value={destination.enabled ? "enabled" : "disabled"}
        />
      </SurfacePanel>

      <PublishDestinationBindingsSection
        buildTargets={buildTargets}
        destination={destination}
        destinationErrors={destinationErrors}
        disabled={disabled}
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
            buttonLabel="Pick folder"
            dialogTitle="Select publish destination folder"
            disabled={bindingDisabled}
            error={bindingErrors.filesystemDirectoryPath}
            hint="The artifact will move into this absolute directory when the binding succeeds."
            label="Destination directory"
            onPathPicked={(path) =>
              handleBindingFieldChange({
                filesystemDirectoryPath: path,
              })
            }
            pickerKind="directory"
            placeholder="D:/Published/Windows"
            value={binding.filesystemDirectoryPath}
          />
        )}
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
  onAddBinding,
  onBindingChange,
  onDestinationChange,
  onOpenCredentialComposer,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
}: PublishDestinationAdapterComponentProps) {
  const publishCredentialOptions = buildPublishCredentialOptions(
    credentials,
    destination.credentialsId,
  );

  return (
    <>
      <SurfacePanel
        bodyClassName="wizard-form-grid"
        className="project-detail-form-grid__span-full"
        description="Define the operator-facing identity and host executable path for this Itch publish destination."
        headerSeparated
        title="Destination identity"
        tone="inset"
      >
        <SelectField
          label="Status"
          onChange={(event) =>
            onDestinationChange({
              enabled: event.currentTarget.value === "enabled",
            })
          }
          options={DESTINATION_STATUS_OPTIONS}
          value={destination.enabled ? "enabled" : "disabled"}
        />

        <TextField
          error={destinationErrors.itchAccountName}
          label="Itch account name"
          onChange={(event) =>
            onDestinationChange({
              itchAccountName: event.currentTarget.value,
            })
          }
          placeholder="indiegabo"
          value={destination.itchAccountName}
        />

        <TextField
          error={destinationErrors.itchGameSlug}
          label="Itch game slug"
          onChange={(event) =>
            onDestinationChange({
              itchGameSlug: event.currentTarget.value,
            })
          }
          placeholder="revolutions"
          value={destination.itchGameSlug}
        />

        <PathPickerField
          buttonLabel="Pick butler"
          clearLabel="Reset"
          clearable
          dialogTitle="Select butler executable"
          disabled={disabled}
          error={destinationErrors.itchButlerPath}
          filters={[
            {
              name: "Butler",
              extensions: ["exe", "cmd", "bat", "sh"],
            },
          ]}
          label="Butler executable"
          onClear={() =>
            onDestinationChange({
              itchButlerPath: "",
            })
          }
          onPathPicked={(path) =>
            onDestinationChange({
              itchButlerPath: path,
            })
          }
          pickerKind="file"
          value={destination.itchButlerPath}
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
              New credential
            </Button>
          ) : null
        }
        className="project-detail-form-grid__span-full"
        description="Choose the stored publish credential that should back Butler uploads for this destination."
        headerSeparated
        title="Credential state"
        tone="inset"
      >
        <SelectField
          error={destinationErrors.credentialsId}
          label="Credential"
          onChange={(event) =>
            onDestinationChange({
              credentialsId: event.currentTarget.value
                ? Number(event.currentTarget.value)
                : null,
            })
          }
          options={publishCredentialOptions}
          value={destination.credentialsId?.toString() ?? ""}
        />
      </SurfacePanel>

      <PublishDestinationBindingsSection
        buildTargets={buildTargets}
        destination={destination}
        destinationErrors={destinationErrors}
        disabled={disabled}
        onAddBinding={onAddBinding}
        onBindingChange={onBindingChange}
        onPendingBindingTargetChange={onPendingBindingTargetChange}
        onRemoveBinding={onRemoveBinding}
        pendingBindingTargetId={pendingBindingTargetId}
        renderBindingFields={({
          binding,
          bindingErrors,
          onBindingChange: handleBindingFieldChange,
        }) => (
          <>
            <TextField
              error={bindingErrors.itchChannel}
              hint="Use the Itch channel that should receive this build target artifact."
              label="Itch channel"
              onChange={(event) =>
                handleBindingFieldChange({
                  itchChannel: event.currentTarget.value,
                })
              }
              value={binding.itchChannel}
            />

            <TextField
              hint="Optional template. Leave empty to use the git tag as the userversion."
              label="Itch userversion template"
              onChange={(event) =>
                handleBindingFieldChange({
                  itchUserversionTemplate: event.currentTarget.value,
                })
              }
              placeholder="{{git_tag}}"
              value={binding.itchUserversionTemplate}
            />
          </>
        )}
      />
    </>
  );
}

function PublishDestinationBindingsSection({
  buildTargets,
  destination,
  destinationErrors,
  disabled,
  onAddBinding,
  onBindingChange,
  onPendingBindingTargetChange,
  onRemoveBinding,
  pendingBindingTargetId,
  renderBindingFields,
}: PublishDestinationBindingsSectionProps) {
  const { openOverlay } = useOverlay();
  const availableBindingTargets = buildTargets.filter(
    (target) =>
      !destination.bindings.some(
        (binding) => binding.buildTargetDraftId === target.id,
      ),
  );
  const pendingBindingTarget = availableBindingTargets.find(
    (target) => target.id === pendingBindingTargetId,
  );
  const shouldUseBindingSelectorOverlay =
    availableBindingTargets.length >= BINDING_SELECTOR_OVERLAY_THRESHOLD;

  const handleOpenBindingTargetSelector = async () => {
    if (disabled || availableBindingTargets.length === 0) {
      return;
    }

    const selectedTargetId = await openOverlay<string>(SelectListFullScreen, {
      description:
        "Search the unbound build targets and select one result to bind to this publish destination.",
      emptyStateCopy:
        "Every currently available build target is already bound to this destination.",
      emptyStateTitle: "No unbound build targets are available.",
      items: buildBindingTargetItems(availableBindingTargets),
      title: "Select build target",
    });

    if (selectedTargetId) {
      onPendingBindingTargetChange(selectedTargetId);
    }
  };

  return (
    <SurfacePanel
      bodyClassName="wizard-form-grid"
      className="project-detail-form-grid__span-full"
      description="Bind build targets to this destination and configure the per-target delivery fields each binding requires."
      headerSeparated
      title="Target bindings"
      tone="inset"
    >
      <div className="project-detail-form-grid__span-full">
        <div className="project-detail-target-card">
          <div className="wizard-callout__actions">
            {shouldUseBindingSelectorOverlay ? (
              <Button
                disabled={disabled || availableBindingTargets.length === 0}
                leadingIcon="search"
                onClick={() => {
                  void handleOpenBindingTargetSelector();
                }}
                size="sm"
                variant="ghost"
              >
                {pendingBindingTarget
                  ? `Target: ${pendingBindingTarget.name}`
                  : "Select target"}
              </Button>
            ) : (
              <SelectField
                disabled={disabled || availableBindingTargets.length === 0}
                label="Target"
                onChange={(event) =>
                  onPendingBindingTargetChange(event.currentTarget.value)
                }
                options={buildBindingTargetOptions(availableBindingTargets)}
                value={pendingBindingTargetId}
              />
            )}
            <Button
              disabled={
                disabled ||
                availableBindingTargets.length === 0 ||
                !pendingBindingTargetId
              }
              leadingIcon="plus"
              onClick={onAddBinding}
              size="sm"
              variant="secondary"
            >
              Add target
            </Button>
          </div>

          {destinationErrors.bindingsRoot ? (
            <p className="ui-field__error">{destinationErrors.bindingsRoot}</p>
          ) : null}
        </div>
      </div>

      <div className="project-detail-form-grid__span-full">
        {destination.bindings.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">No bound build targets.</p>
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
                  <div className="project-detail-target-card__header">
                    <div className="project-detail-target-card__title-block">
                      <h4 className="project-detail-target-card__title">
                        {buildTarget?.name || binding.buildTargetName}
                      </h4>
                    </div>

                    <div className="project-detail-target-card__badges">
                      <IconButton
                        className="wizard-target-card__remove"
                        disabled={disabled}
                        icon="trash"
                        label={`Remove binding for ${
                          buildTarget?.name || binding.buildTargetName
                        }`}
                        onClick={() => onRemoveBinding(binding.id)}
                        size="sm"
                        variant="ghost"
                      />
                    </div>
                  </div>

                  <div className="wizard-form-grid">
                    <SelectField
                      label="Status"
                      onChange={(event) =>
                        onBindingChange(binding.id, {
                          enabled: event.currentTarget.value === "enabled",
                        })
                      }
                      options={BINDING_STATUS_OPTIONS}
                      value={binding.enabled ? "enabled" : "disabled"}
                    />

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
    itchButlerPath: "",
    credentialsId: null,
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
      itchButlerPath: readJsonStringField(config, "butler_path") || "",
      credentialsId: publishTarget.credentials?.credential_id ?? null,
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
      if (
        destination.itchButlerPath.trim() &&
        !looksLikeAbsolutePath(destination.itchButlerPath)
      ) {
        destinationErrors.itchButlerPath =
          "Butler path must be absolute when provided.";
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
      }

      if (destination.kind === "itch") {
        if (!binding.itchChannel.trim()) {
          bindingErrors.itchChannel = "Itch channel is required.";
        } else if (binding.itchChannel.includes(":")) {
          bindingErrors.itchChannel = "Itch channel must not contain ':'.";
        }
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
      usesHostTransportProbe:
        destination.kind === "itch" && !destination.itchButlerPath.trim(),
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
  buildTargets: ProjectBuildTargetReference[],
): SelectOption[] {
  return [
    {
      disabled: buildTargets.length === 0,
      label:
        buildTargets.length === 0
          ? "No unbound build targets available"
          : "Select a build target",
      value: "",
    },
    ...buildTargets.map((target) => ({
      label: target.name,
      value: target.id,
    })),
  ];
}

function buildBindingTargetItems(
  buildTargets: ProjectBuildTargetReference[],
): SelectListItem[] {
  return buildTargets.map((target) => ({
    id: target.id,
    label: target.name,
    subtitle: target.buildTargetId
      ? `Build target id ${target.buildTargetId}`
      : "Draft-only build target",
  }));
}

function buildPublishCredentialOptions(
  credentials: SecretCredentialSetting[],
  selectedCredentialId: number | null,
): SelectOption[] {
  const selectableCredentials = credentials.filter(isItchCredentialSelectable);
  const options: SelectOption[] = [
    {
      label:
        selectableCredentials.length === 0
          ? "No stored Itch credentials available"
          : "No Itch credential selected",
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
    options.push({
      label: `Current credential #${selectedCredentialId}`,
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
  if (destination.itchButlerPath.trim()) {
    config.butler_path = destination.itchButlerPath.trim();
  }
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

function formatPublishDestinationKindLabel(kind: PublishDestinationKind) {
  return kind === "filesystem" ? "Folder" : "Itch";
}

function formatPublishDestinationTitle(destination: PublishDestinationDraft) {
  return derivePublishDestinationName(destination.kind);
}

function derivePublishDestinationName(kind: PublishDestinationKind) {
  return formatPublishDestinationKindLabel(kind);
}

function listPublishDestinationKinds(): PublishDestinationKind[] {
  return ["filesystem", "itch"];
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
    errors.itchButlerPath ||
    errors.bindingsRoot ||
    Object.values(errors.bindings).some((bindingErrors) =>
      hasPublishDestinationBindingErrors(bindingErrors),
    ),
  );
}

function hasPublishDestinationBindingErrors(
  errors: PublishDestinationBindingErrors,
) {
  return Boolean(
    errors.buildTarget || errors.filesystemDirectoryPath || errors.itchChannel,
  );
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
