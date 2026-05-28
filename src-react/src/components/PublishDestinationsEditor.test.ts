import { describe, expect, it } from "vitest";

import {
    buildPublishDestinationDrafts,
    buildUpdateProjectPublishTargetsInput,
    collectBuildTargetBindingImpact,
    createEmptyPublishDestinationDraft,
    listUnboundBuildTargetNames,
    removeBuildTargetBindings,
    validatePublishDestinationDrafts,
    type ProjectBuildTargetReference,
    type PublishDestinationBindingDraft,
    type PublishDestinationDraft,
} from "./PublishDestinationsEditor";

const BUILD_TARGETS: ProjectBuildTargetReference[] = [
    {
        id: "target-windows",
        buildTargetId: 11,
        name: "Windows",
    },
    {
        id: "target-linux",
        buildTargetId: 22,
        name: "Linux",
    },
];

describe("PublishDestinationsEditor helpers", () => {
    it("keeps zero-destination projects valid and returns all targets as unbound", () => {
        const errors = validatePublishDestinationDrafts([], BUILD_TARGETS);

        expect(errors.root).toBeUndefined();
        expect(listUnboundBuildTargetNames([], BUILD_TARGETS)).toEqual([
            "Linux",
            "Windows",
        ]);
        expect(buildUpdateProjectPublishTargetsInput([], BUILD_TARGETS)).toEqual([]);
    });

    it("rejects more than one enabled consuming binding for the same build target", () => {
        const primaryBinding = createFilesystemBinding(BUILD_TARGETS[0], "D:/published/primary");
        const backupBinding = createFilesystemBinding(BUILD_TARGETS[0], "D:/published/backup");
        const destinations = [
            createFilesystemDestination("Move primary", primaryBinding),
            createFilesystemDestination("Move backup", backupBinding),
        ];

        const errors = validatePublishDestinationDrafts(destinations, BUILD_TARGETS);

        expect(errors.root).toContain("Windows");
        expect(
            errors.destinations[destinations[0].id]?.bindings[primaryBinding.id]
                ?.buildTarget,
        ).toContain("Only one enabled consuming binding");
        expect(
            errors.destinations[destinations[1].id]?.bindings[backupBinding.id]
                ?.buildTarget,
        ).toContain("Only one enabled consuming binding");
    });

    it("parses persisted destination inspection and rebuilds update payloads", () => {
        const drafts = buildPublishDestinationDrafts(
            [
                {
                    publish_target_id: 5,
                    name: "Itch stable",
                    kind: "itch",
                    enabled: true,
                    config_json: JSON.stringify({
                        account_name: "indiegabo",
                        game_slug: "red-horizon",
                    }),
                    credentials: {
                        credential_id: 90,
                        name: "Itch main",
                        kind: "itch-api-key",
                        config_status: "ready",
                        config_message: "Ready",
                    },
                    bindings: [
                        {
                            build_target_id: 11,
                            build_target_name: "Windows",
                            enabled: true,
                            options_json: JSON.stringify({
                                channel: "windows-stable",
                                userversion_template: "{{git_tag}}",
                            }),
                            consumption_behavior: "non_consuming",
                        },
                    ],
                },
            ],
            BUILD_TARGETS,
        );

        expect(drafts).toHaveLength(1);
        expect(drafts[0]?.name).toBe("Itch");
        expect(drafts[0]?.itchAccountName).toBe("indiegabo");
        expect(drafts[0]?.bindings[0]?.buildTargetDraftId).toBe("target-windows");
        expect(drafts[0]?.bindings[0]?.itchChannel).toBe("windows-stable");

        const payload = buildUpdateProjectPublishTargetsInput(drafts, BUILD_TARGETS);

        expect(payload).toEqual([
            {
                publish_target_id: 5,
                name: "Itch",
                kind: "itch",
                enabled: true,
                config_json: JSON.stringify({
                    account_name: "indiegabo",
                    game_slug: "red-horizon",
                }),
                credentials_id: 90,
                bindings: [
                    {
                        build_target_id: 11,
                        build_target_name: "Windows",
                        enabled: true,
                        options_json: JSON.stringify({
                            channel: "windows-stable",
                            userversion_template: "{{git_tag}}",
                        }),
                    },
                ],
            },
        ]);
    });

    it("reports removal impact and strips bindings for removed build targets", () => {
        const destinations = [
            createFilesystemDestination(
                "Move Windows",
                createFilesystemBinding(BUILD_TARGETS[0], "D:/published/windows"),
            ),
            createItchDestination(
                "Itch Linux",
                createItchBinding(BUILD_TARGETS[1], "linux-stable"),
            ),
        ];

        expect(
            collectBuildTargetBindingImpact(destinations, BUILD_TARGETS[0].id),
        ).toEqual(["Folder"]);

        const withoutWindows = removeBuildTargetBindings(
            destinations,
            BUILD_TARGETS[0].id,
        );

        expect(withoutWindows[0]?.bindings).toHaveLength(0);
        expect(listUnboundBuildTargetNames(withoutWindows, BUILD_TARGETS)).toEqual([
            "Windows",
        ]);
    });

    it("rejects duplicate destination adapter kinds", () => {
        const destinations = [
            createFilesystemDestination(
                "Move primary",
                createFilesystemBinding(BUILD_TARGETS[0], "D:/published/primary"),
            ),
            createFilesystemDestination(
                "Move backup",
                createFilesystemBinding(BUILD_TARGETS[1], "D:/published/backup"),
            ),
        ];

        const errors = validatePublishDestinationDrafts(destinations, BUILD_TARGETS);

        expect(errors.root).toContain("Folder is already added");
    });
});

function createFilesystemDestination(
    name: string,
    binding: PublishDestinationBindingDraft,
): PublishDestinationDraft {
    return {
        ...createEmptyPublishDestinationDraft(),
        name,
        kind: "filesystem",
        bindings: [binding],
    };
}

function createItchDestination(
    name: string,
    binding: PublishDestinationBindingDraft,
): PublishDestinationDraft {
    return {
        ...createEmptyPublishDestinationDraft(),
        name,
        kind: "itch",
        itchAccountName: "indiegabo",
        itchGameSlug: "red-horizon",
        bindings: [binding],
    };
}

function createFilesystemBinding(
    buildTarget: ProjectBuildTargetReference,
    directoryPath: string,
): PublishDestinationBindingDraft {
    return {
        id: `${buildTarget.id}-filesystem-binding`,
        buildTargetDraftId: buildTarget.id,
        buildTargetId: buildTarget.buildTargetId,
        buildTargetName: buildTarget.name,
        enabled: true,
        filesystemDirectoryPath: directoryPath,
        itchChannel: "",
        itchUserversionTemplate: "",
    };
}

function createItchBinding(
    buildTarget: ProjectBuildTargetReference,
    channel: string,
): PublishDestinationBindingDraft {
    return {
        id: `${buildTarget.id}-itch-binding`,
        buildTargetDraftId: buildTarget.id,
        buildTargetId: buildTarget.buildTargetId,
        buildTargetName: buildTarget.name,
        enabled: true,
        filesystemDirectoryPath: "",
        itchChannel: channel,
        itchUserversionTemplate: "",
    };
}