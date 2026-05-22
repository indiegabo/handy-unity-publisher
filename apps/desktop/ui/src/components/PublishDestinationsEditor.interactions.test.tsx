import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import OverlayProvider from "./OverlayManager";
import {
  PublishDestinationsEditor,
  createEmptyPublishDestinationDraft,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
} from "./PublishDestinationsEditor";

const BUILD_TARGETS: ProjectBuildTargetReference[] = [
  {
    id: "target-windows",
    buildTargetId: 11,
    name: "Windows",
  },
];

afterEach(() => {
  cleanup();
});

describe("PublishDestinationsEditor interactions", () => {
  it("adds a destination through the overlay and returns to a quick view card", async () => {
    render(<Harness editingMode="overlay" initialDestinations={[]} />);

    fireEvent.click(screen.getByRole("button", { name: "Add destination" }));

    expect(
      screen.getByRole("menu", { name: "Destination list" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Itch" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add Itch destination",
    });

    expect(within(dialog).getByLabelText("Itch account name")).toHaveFocus();
    expect(
      within(dialog).queryByText(
        "Configure one publish destination, then return to the wizard with a compact quick view.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByLabelText("Destination type"),
    ).not.toBeInTheDocument();
    expect(within(dialog).queryByLabelText("Status")).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText(
        "Define the operator-facing identity and host executable path for this Itch publish destination.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText(
        "Choose the stored publish credential that should back Butler uploads for this destination.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText(
        "Bind build targets to this destination and configure the per-target delivery fields each binding requires.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).getByRole("heading", { name: "Access" }),
    ).toBeInTheDocument();

    fireEvent.change(
      await within(dialog).findByLabelText("Itch account name"),
      {
        target: { value: "indiegabo" },
      },
    );
    fireEvent.change(within(dialog).getByLabelText("Itch game slug"), {
      target: { value: "red-horizon" },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Add target" }));
    expect(within(dialog).queryByLabelText("Status")).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText(
        "Use the Itch channel that should receive this build target artifact.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText(
        "Optional template. Leave empty to use the git tag as the userversion.",
      ),
    ).not.toBeInTheDocument();

    fireEvent.change(await within(dialog).findByLabelText("Itch channel"), {
      target: { value: "windows" },
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add destination" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Add Itch destination" }),
      ).not.toBeInTheDocument();
    });

    expect(screen.getByRole("heading", { name: "Itch" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
    expect(
      screen.queryByLabelText("Itch account name"),
    ).not.toBeInTheDocument();
  });

  it("omits destination status controls when creating a folder destination", async () => {
    render(<Harness editingMode="overlay" initialDestinations={[]} />);

    fireEvent.click(screen.getByRole("button", { name: "Add destination" }));
    fireEvent.click(screen.getByRole("button", { name: "Folder" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add Folder destination",
    });

    expect(within(dialog).queryByLabelText("Status")).not.toBeInTheDocument();
    expect(
      within(dialog).queryByRole("heading", { name: "Destination identity" }),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).getByRole("heading", { name: "Target bindings" }),
    ).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Target")).toHaveFocus();

    fireEvent.click(within(dialog).getByRole("button", { name: "Add target" }));

    expect(within(dialog).queryByLabelText("Status")).not.toBeInTheDocument();
  });

  it("reopens an existing destination in the overlay with its current data", async () => {
    render(
      <Harness
        editingMode="overlay"
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            bindings: [
              {
                id: "binding-windows",
                buildTargetDraftId: BUILD_TARGETS[0].id,
                buildTargetId: BUILD_TARGETS[0].buildTargetId,
                buildTargetName: BUILD_TARGETS[0].name,
                enabled: true,
                filesystemDirectoryPath: "D:/published/windows",
                itchChannel: "",
                itchUserversionTemplate: "",
              },
            ],
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Edit Folder destination",
    });

    expect(
      within(dialog).getByDisplayValue("D:/published/windows"),
    ).toBeInTheDocument();

    fireEvent.change(within(dialog).getAllByLabelText("Status")[0], {
      target: { value: "disabled" },
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Save destination" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Edit Folder destination" }),
      ).not.toBeInTheDocument();
    });

    expect(screen.getByText("Disabled")).toBeInTheDocument();
  });

  it("opens the destination list, blocks duplicates, and adds the clicked destination", () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add destination" }));

    expect(
      screen.getByRole("menu", { name: "Destination list" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Folder" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Itch" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Itch" }));

    const itchAccordion = screen
      .getByRole("heading", { name: "Itch" })
      .closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    fireEvent.click(
      within(itchAccordion as HTMLElement).getByRole("button", {
        name: "Expand section",
      }),
    );

    expect(screen.getByLabelText("Itch account name")).toBeInTheDocument();
    expect(
      within(itchAccordion as HTMLElement).getByRole("heading", {
        name: "Destination identity",
      }),
    ).toBeInTheDocument();
    expect(
      within(itchAccordion as HTMLElement).getByRole("heading", {
        name: "Credential state",
      }),
    ).toBeInTheDocument();
    expect(
      within(itchAccordion as HTMLElement).getByRole("heading", {
        name: "Target bindings",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Remove Itch destination" }),
    ).toBeInTheDocument();
  });

  it("binds the default available target without requiring a manual selection change", () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            name: "Windows Folder",
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    expect(
      screen.queryByText("No bound build targets."),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Remove binding for Windows" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No unbound build targets available."),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Add target" }),
    ).not.toBeInTheDocument();
  });

  it("asks for confirmation before removing a destination with bindings", () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            name: "Move Windows",
            bindings: [
              {
                id: "binding-windows",
                buildTargetDraftId: BUILD_TARGETS[0].id,
                buildTargetId: BUILD_TARGETS[0].buildTargetId,
                buildTargetName: BUILD_TARGETS[0].name,
                enabled: true,
                filesystemDirectoryPath: "D:/published/windows",
                itchChannel: "",
                itchUserversionTemplate: "",
              },
            ],
          },
        ]}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Remove Folder destination" }),
    );

    expect(screen.getByText("Confirm destination removal")).toBeInTheDocument();
    expect(
      screen.getByText(/also removes persisted bindings for/i),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(
      screen.queryByText("Confirm destination removal"),
    ).not.toBeInTheDocument();
  });

  it("shows both destination kinds as locked once they are already added", () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft("filesystem"),
          },
          {
            ...createEmptyPublishDestinationDraft("itch"),
            credentialsId: 90,
            itchAccountName: "indiegabo",
            itchGameSlug: "red-horizon",
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add destination" }));

    expect(screen.getByRole("button", { name: "Folder" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Itch" })).toBeDisabled();
  });

  it("keeps a compact summary visible when a destination accordion is collapsed", () => {
    const buildTargets = [
      {
        buildTargetId: 11,
        id: "target-android",
        name: "Android",
      },
      {
        buildTargetId: 22,
        id: "target-linux",
        name: "Linux",
      },
      {
        buildTargetId: 33,
        id: "target-windows",
        name: "Windows",
      },
    ];

    render(
      <Harness
        buildTargets={buildTargets}
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft("itch"),
            bindings: [
              {
                id: "binding-android",
                buildTargetDraftId: buildTargets[0].id,
                buildTargetId: buildTargets[0].buildTargetId,
                buildTargetName: buildTargets[0].name,
                enabled: true,
                filesystemDirectoryPath: "",
                itchChannel: "android-beta",
                itchUserversionTemplate: "",
              },
              {
                id: "binding-linux",
                buildTargetDraftId: buildTargets[1].id,
                buildTargetId: buildTargets[1].buildTargetId,
                buildTargetName: buildTargets[1].name,
                enabled: true,
                filesystemDirectoryPath: "",
                itchChannel: "linux-beta",
                itchUserversionTemplate: "",
              },
              {
                id: "binding-windows",
                buildTargetDraftId: buildTargets[2].id,
                buildTargetId: buildTargets[2].buildTargetId,
                buildTargetName: buildTargets[2].name,
                enabled: true,
                filesystemDirectoryPath: "",
                itchChannel: "windows-beta",
                itchUserversionTemplate: "",
              },
            ],
            itchAccountName: "indiegabo",
            itchGameSlug: "red-horizon",
          },
        ]}
      />,
    );

    const itchAccordion = screen
      .getByRole("heading", { name: "Itch" })
      .closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    fireEvent.click(
      within(itchAccordion as HTMLElement).getByRole("button", {
        name: "Collapse section",
      }),
    );

    expect(itchAccordion).toHaveAttribute("data-state", "closed");
    expect(
      (itchAccordion as HTMLElement).querySelector(
        ".publish-destination-card__summary-strip.ui-summary-strip",
      ),
    ).not.toBeNull();
    expect(
      within(itchAccordion as HTMLElement).getByText("3 targets"),
    ).toBeInTheDocument();
    expect(
      within(itchAccordion as HTMLElement).getByText("Android, Linux +1 more"),
    ).toBeInTheDocument();
    expect(
      within(itchAccordion as HTMLElement).getByText("Missing"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Itch account name")).not.toBeVisible();
  });

  it("opens the binding target overlay when many unbound targets are available", async () => {
    const buildTargets = Array.from({ length: 9 }, (_, index) => ({
      buildTargetId: index + 1,
      id: `target-${index + 1}`,
      name: `Target ${index + 1}`,
    }));

    render(
      <Harness
        buildTargets={buildTargets}
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            name: "Large Inventory Folder",
          },
        ]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Target: Target 1" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Select build target",
    });

    fireEvent.click(
      within(dialog)
        .getByText("Target 9")
        .closest("button") as HTMLButtonElement,
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Target: Target 9" }),
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    expect(
      screen.getByRole("button", { name: "Remove binding for Target 9" }),
    ).toBeInTheDocument();
  });

  it("autofocuses the binding selector filter and restores focus on Escape", async () => {
    const buildTargets = Array.from({ length: 9 }, (_, index) => ({
      buildTargetId: index + 1,
      id: `target-${index + 1}`,
      name: `Target ${index + 1}`,
    }));

    render(
      <Harness
        buildTargets={buildTargets}
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            name: "Large Inventory Folder",
          },
        ]}
      />,
    );

    const trigger = screen.getByRole("button", { name: "Target: Target 1" });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "Select build target",
    });
    const filterInput = within(dialog).getByPlaceholderText(
      "Search by name or secondary text",
    );

    expect(filterInput).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Select build target" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });
  });

  it("closes the binding selector from its close button and restores focus to the trigger", async () => {
    const buildTargets = Array.from({ length: 9 }, (_, index) => ({
      buildTargetId: index + 1,
      id: `target-${index + 1}`,
      name: `Target ${index + 1}`,
    }));

    render(
      <Harness
        buildTargets={buildTargets}
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft(),
            name: "Large Inventory Folder",
          },
        ]}
      />,
    );

    const trigger = screen.getByRole("button", { name: "Target: Target 1" });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "Select build target",
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Select build target" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });
  });

  it("opens the credential composer overlay and saves the returned payload", async () => {
    const onSaveCredential = vi.fn().mockResolvedValue(undefined);

    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft("itch"),
            id: "destination-itch",
            itchAccountName: "indiegabo",
            itchGameSlug: "red-horizon",
            name: "Itch Release",
          },
        ]}
        onSaveCredential={onSaveCredential}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New credential" }));

    const dialog = await screen.findByRole("dialog", {
      name: "New publish credential",
    });

    const credentialNameInput = within(dialog)
      .getByText("Credential name")
      .closest("label")
      ?.querySelector("input");
    const apiKeyInput = within(dialog)
      .getByText("API key")
      .closest("label")
      ?.querySelector("input");

    expect(credentialNameInput).not.toBeNull();
    expect(apiKeyInput).not.toBeNull();

    fireEvent.change(credentialNameInput as HTMLInputElement, {
      target: { value: "Itch Release Key" },
    });
    fireEvent.change(apiKeyInput as HTMLInputElement, {
      target: { value: "butler-secret" },
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Save credential" }),
    );

    await waitFor(() => {
      expect(onSaveCredential).toHaveBeenCalledWith("destination-itch", {
        credential_id: null,
        config_json: JSON.stringify({ api_key: "butler-secret" }),
        kind: "itch-api-key",
        name: "Itch Release Key",
      });
    });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "New publish credential" }),
      ).not.toBeInTheDocument();
    });
  });

  it("autofocuses the credential name field and restores focus on cancel", async () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft("itch"),
            id: "destination-itch",
            itchAccountName: "indiegabo",
            itchGameSlug: "red-horizon",
            name: "Itch Release",
          },
        ]}
        onSaveCredential={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const trigger = screen.getByRole("button", { name: "New credential" });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "New publish credential",
    });
    const credentialNameInput = within(dialog)
      .getByText("Credential name")
      .closest("label")
      ?.querySelector("input");

    expect(credentialNameInput).not.toBeNull();

    expect(credentialNameInput as HTMLInputElement).toHaveFocus();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "New publish credential" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });
  });

  it("closes the credential composer from its close button and restores focus to the trigger", async () => {
    render(
      <Harness
        initialDestinations={[
          {
            ...createEmptyPublishDestinationDraft("itch"),
            id: "destination-itch",
            itchAccountName: "indiegabo",
            itchGameSlug: "red-horizon",
            name: "Itch Release",
          },
        ]}
        onSaveCredential={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const trigger = screen.getByRole("button", { name: "New credential" });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "New publish credential",
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "New publish credential" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });
  });
});

function Harness({
  buildTargets = BUILD_TARGETS,
  editingMode = "inline",
  initialDestinations,
  onSaveCredential,
}: {
  buildTargets?: ProjectBuildTargetReference[];
  editingMode?: "inline" | "overlay";
  initialDestinations: PublishDestinationDraft[];
  onSaveCredential?: (
    destinationId: string,
    input: Record<string, unknown>,
  ) => Promise<void> | void;
}) {
  const [destinations, setDestinations] = useState(initialDestinations);

  return (
    <OverlayProvider>
      <PublishDestinationsEditor
        buildTargets={buildTargets}
        credentials={[]}
        destinations={destinations}
        editingMode={editingMode}
        onChange={setDestinations}
        onSaveCredential={onSaveCredential}
      />
    </OverlayProvider>
  );
}
