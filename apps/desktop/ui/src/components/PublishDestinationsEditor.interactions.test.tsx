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
});

function Harness({
  buildTargets = BUILD_TARGETS,
  initialDestinations,
  onSaveCredential,
}: {
  buildTargets?: ProjectBuildTargetReference[];
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
        onChange={setDestinations}
        onSaveCredential={onSaveCredential}
      />
    </OverlayProvider>
  );
}
