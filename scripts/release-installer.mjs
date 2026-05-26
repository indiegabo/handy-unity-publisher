import { spawnSync } from "node:child_process";
import {
    copyFileSync,
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    rmSync,
    statSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptFilePath = fileURLToPath(import.meta.url);
const scriptDirectory = path.dirname(scriptFilePath);
const repositoryRoot = path.resolve(scriptDirectory, "..");

const targetTriple = process.env.HGP_RELEASE_TARGET || "x86_64-pc-windows-msvc";
const buildProfile = "release";
const bundleDirectory = path.join(
    repositoryRoot,
    "tmp",
    "cargo-targets",
    "default",
    targetTriple,
    buildProfile,
    "bundle",
);
const releasesDirectory = path.join(repositoryRoot, "releases");
const tauriConfigPath = path.join(
    repositoryRoot,
    "apps",
    "desktop",
    "src-tauri",
    "tauri.conf.json",
);
const tauriReleaseConfigPath = path.join(
    repositoryRoot,
    "apps",
    "desktop",
    "src-tauri",
    "tauri.release.conf.json",
);

const tauriConfig = readJsonFile(tauriConfigPath);
const tauriReleaseConfig = readJsonFile(tauriReleaseConfigPath);

const artifactBaseName = sanitizeArtifactName(
    process.env.HGP_RELEASE_ARTIFACT_APP_NAME ||
    tauriReleaseConfig.productName ||
    tauriConfig.productName ||
    "HandyGamesPublisher",
);
const artifactVersionSegment = normalizeVersionForArtifact(
    process.env.HGP_RELEASE_VERSION || tauriConfig.version,
);

cleanDirectory(path.join(bundleDirectory, "msi"));
cleanDirectory(path.join(bundleDirectory, "nsis"));
cleanDirectory(releasesDirectory);

const tauriConfigOverride = JSON.stringify({
    build: {
        beforeBuildCommand:
            "node ./scripts/sync-app-version.mjs && npm run build --prefix apps/desktop/ui",
    },
});

runCommand(
    "powershell.exe",
    [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "./scripts/prepare-tauri-sidecar.ps1",
        "-TargetTriple",
        targetTriple,
        "-BuildProfile",
        buildProfile,
    ],
    "prepare tauri runtime sidecar",
);

runCommand(
    "node",
    ["./scripts/prepare-butler-sidecar.mjs", "--target", targetTriple],
    "prepare butler sidecar",
);

runCommand(
    "cargo",
    [
        "tauri",
        "build",
        "--config",
        "apps/desktop/src-tauri/tauri.release.conf.json",
        "--config",
        tauriConfigOverride,
        "--target",
        targetTriple,
    ],
    "build desktop installer bundles",
);

const copiedArtifacts = copyBundleArtifacts(
    bundleDirectory,
    releasesDirectory,
    artifactBaseName,
    artifactVersionSegment,
);

if (copiedArtifacts.length === 0) {
    throw new Error(
        `No bundle artifacts were found under '${bundleDirectory}'.`,
    );
}

console.log("Release artifacts copied to ./releases:");
for (const artifactPath of copiedArtifacts) {
    console.log(` - ${path.relative(repositoryRoot, artifactPath)}`);
}

function copyBundleArtifacts(
    sourceRoot,
    destinationRoot,
    baseName,
    versionSegment,
) {
    const subdirectories = ["msi", "nsis"];
    mkdirSync(destinationRoot, { recursive: true });

    const copied = [];
    const seenExtensions = new Set();
    for (const subdirectory of subdirectories) {
        const sourceDirectory = path.join(sourceRoot, subdirectory);
        if (!existsSync(sourceDirectory)) {
            continue;
        }

        for (const entry of readdirSync(sourceDirectory)) {
            const sourcePath = path.join(sourceDirectory, entry);
            if (!statSync(sourcePath).isFile()) {
                continue;
            }

            const extension = path.extname(entry).slice(1).toLowerCase();
            if (!extension) {
                continue;
            }

            if (seenExtensions.has(extension)) {
                throw new Error(
                    `Multiple installer artifacts share extension '.${extension}'.`,
                );
            }

            seenExtensions.add(extension);
            const destinationFileName =
                `${baseName}.v${versionSegment}.setup.${extension}`;
            const destinationPath = path.join(destinationRoot, destinationFileName);
            copyFileSync(sourcePath, destinationPath);
            copied.push(destinationPath);
        }
    }

    return copied;
}

function readJsonFile(filePath) {
    return JSON.parse(readFileSync(filePath, "utf8"));
}

function cleanDirectory(directoryPath) {
    rmSync(directoryPath, { recursive: true, force: true });
}

function sanitizeArtifactName(value) {
    const sanitized = value.replace(/\s+/g, "").replace(/[^A-Za-z0-9._-]/g, "");
    if (!sanitized) {
        throw new Error("Artifact app name is empty after sanitization.");
    }

    return sanitized;
}

function normalizeVersionForArtifact(version) {
    if (!version) {
        throw new Error("Artifact version is missing.");
    }

    const normalized = version
        .trim()
        .replace(/\./g, "_")
        .replace(/[^A-Za-z0-9_-]/g, "_")
        .replace(/_+/g, "_");

    if (!normalized) {
        throw new Error("Artifact version is empty after normalization.");
    }

    return normalized;
}

function runCommand(command, args, description) {
    console.log(`\n>> ${description}`);
    const result = spawnSync(command, args, {
        cwd: repositoryRoot,
        stdio: "inherit",
        shell: false,
    });

    if (result.status !== 0) {
        throw new Error(`Command failed: ${command} ${args.join(" ")}`);
    }
}