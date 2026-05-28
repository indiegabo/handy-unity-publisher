import { readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const workspaceVersion = readWorkspaceVersion(
    resolve(repositoryRoot, "Cargo.toml"),
);
const isCheckMode = process.argv.includes("--check");

const synchronizationTargets = [
    {
        filePath: resolve(repositoryRoot, "src-react", "package.json"),
        indentation: 2,
        label: "desktop UI manifest",
        update(document) {
            return updateJsonValue(document, ["version"], workspaceVersion);
        },
    },
    {
        filePath: resolve(
            repositoryRoot,
            "src-react",
            "package-lock.json",
        ),
        indentation: 2,
        label: "desktop UI lockfile",
        update(document) {
            let changed = false;

            changed = updateJsonValue(
                document,
                ["version"],
                workspaceVersion,
            ) || changed;
            changed = updateJsonValue(
                document,
                ["packages", "", "version"],
                workspaceVersion,
            ) || changed;

            return changed;
        },
    },
    {
        filePath: resolve(
            repositoryRoot,
            "src-tauri",
            "tauri.conf.json",
        ),
        indentation: 4,
        label: "desktop shell Tauri config",
        update(document) {
            return updateJsonValue(document, ["version"], workspaceVersion);
        },
    },
    {
        filePath: resolve(repositoryRoot, "package-lock.json"),
        indentation: 2,
        label: "workspace lockfile",
        update(document) {
            return updateJsonValue(
                document,
                ["packages", "src-react", "version"],
                workspaceVersion,
            );
        },
    },
];

const pendingUpdates = synchronizationTargets
    .map((target) => {
        const document = JSON.parse(readFileSync(target.filePath, "utf8"));
        const changed = target.update(document);

        return {
            ...target,
            changed,
            document,
        };
    })
    .filter((target) => target.changed);

if (isCheckMode) {
    if (pendingUpdates.length === 0) {
        console.log(`App version is synchronized to ${workspaceVersion}.`);
        process.exit(0);
    }

    console.error(
        `App version drift detected. Cargo workspace version is ${workspaceVersion}.`,
    );

    for (const target of pendingUpdates) {
        console.error(`- ${relative(repositoryRoot, target.filePath)} (${target.label})`);
    }

    process.exit(1);
}

if (pendingUpdates.length === 0) {
    console.log(`App version is already synchronized to ${workspaceVersion}.`);
    process.exit(0);
}

for (const target of pendingUpdates) {
    writeFileSync(
        target.filePath,
        `${JSON.stringify(target.document, null, target.indentation)}\n`,
    );
}

console.log(`Synchronized app version ${workspaceVersion} in:`);

for (const target of pendingUpdates) {
    console.log(`- ${relative(repositoryRoot, target.filePath)}`);
}

function readWorkspaceVersion(filePath) {
    const lines = readFileSync(filePath, "utf8").split(/\r?\n/u);
    let insideWorkspacePackageSection = false;

    for (const line of lines) {
        const sectionMatch = line.match(/^\s*\[(.+?)\]\s*$/u);

        if (sectionMatch) {
            insideWorkspacePackageSection = sectionMatch[1] === "workspace.package";
            continue;
        }

        if (!insideWorkspacePackageSection) {
            continue;
        }

        const versionMatch = line.match(/^\s*version\s*=\s*"([^"]+)"\s*$/u);

        if (versionMatch) {
            return versionMatch[1];
        }
    }

    throw new Error(
        "Could not resolve [workspace.package].version from Cargo.toml.",
    );
}

function updateJsonValue(document, pathSegments, nextValue) {
    let currentValue = document;

    for (const segment of pathSegments.slice(0, -1)) {
        if (!(segment in currentValue) || currentValue[segment] === null) {
            throw new Error(`Missing JSON path segment: ${pathSegments.join(".")}`);
        }

        currentValue = currentValue[segment];
    }

    const finalSegment = pathSegments[pathSegments.length - 1];

    if (currentValue[finalSegment] === nextValue) {
        return false;
    }

    currentValue[finalSegment] = nextValue;
    return true;
}