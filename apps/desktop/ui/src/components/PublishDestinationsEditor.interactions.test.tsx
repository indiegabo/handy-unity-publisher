import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

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

    expect(screen.getByRole("menu", { name: "Destination list" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Folder" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Itch" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Itch" }));

    expect(screen.getByLabelText("Itch account name")).toBeInTheDocument();
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

    expect(screen.queryByText("No bound build targets.")).not.toBeInTheDocument();
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

    expect(
      screen.getByText("Confirm destination removal"),
    ).toBeInTheDocument();
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
});

function Harness({ initialDestinations }: { initialDestinations: PublishDestinationDraft[] }) {
  const [destinations, setDestinations] = useState(initialDestinations);

  return (
    <PublishDestinationsEditor
      buildTargets={BUILD_TARGETS}
      credentials={[]}
      destinations={destinations}
      onChange={setDestinations}
    />
  );
}