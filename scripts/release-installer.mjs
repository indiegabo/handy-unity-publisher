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
import process from "node:process";
import { fileURLToPath } from "node:url";

import { freezeLocalizationWorkingSet } from "./freeze-localization-working-set.mjs";
import { syncLocalizationPacks } from "./sync-localization-packs.mjs";

const scriptFilePath = fileURLToPath(import.meta.url);
const scriptDirectory = path.dirname(scriptFilePath);
const repositoryRoot = path.resolve(scriptDirectory, "..");
const allowLocalizationScaffoldEnvName =
    "HGP_RELEASE_ALLOW_LOCALIZATION_SCAFFOLD";

export function runReleaseInstaller(options = {}) {
    const activeRepositoryRoot = options.repositoryRoot || repositoryRoot;
    const environment = options.environment || process.env;
    const logger = options.logger || console;

    const targetTriple =
        environment.HGP_RELEASE_TARGET || "x86_64-pc-windows-msvc";
    const buildProfile = "release";
    const bundleDirectory = path.join(
        activeRepositoryRoot,
        "tmp",
        "cargo-targets",
        "default",
        targetTriple,
        buildProfile,
        "bundle",
    );
    const releasesDirectory = path.join(activeRepositoryRoot, "releases");
    const tauriConfigPath = path.join(
        activeRepositoryRoot,
        "src-tauri",
        "tauri.conf.json",
    );
    const tauriReleaseConfigPath = path.join(
        activeRepositoryRoot,
        "src-tauri",
        "tauri.release.conf.json",
    );

    const tauriConfig = readJsonFile(tauriConfigPath);
    const tauriReleaseConfig = readJsonFile(tauriReleaseConfigPath);

    const artifactBaseName = sanitizeArtifactName(
        environment.HGP_RELEASE_ARTIFACT_APP_NAME ||
        tauriReleaseConfig.productName ||
        tauriConfig.productName ||
        "HandyGamesPublisher",
    );
    const releaseVersionTag = normalizeVersionForTag(
        environment.HGP_RELEASE_VERSION || tauriConfig.version,
    );
    const artifactVersionSegment = normalizeVersionForArtifact(
        releaseVersionTag,
    );

    runReleaseLocalizationPreflight(activeRepositoryRoot, {
        environment,
        logger,
    });

    cleanDirectory(path.join(bundleDirectory, "msi"));
    cleanDirectory(path.join(bundleDirectory, "nsis"));
    cleanDirectory(releasesDirectory);

    const tauriConfigOverride = JSON.stringify({
        build: {
            beforeBuildCommand:
                "node ./scripts/sync-app-version.mjs && npm run build --prefix src-react",
        },
    });

    runCommand(
        activeRepositoryRoot,
        logger,
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
        activeRepositoryRoot,
        logger,
        "node",
        ["./scripts/prepare-butler-sidecar.mjs", "--target", targetTriple],
        "prepare butler sidecar",
    );

    runCommand(
        activeRepositoryRoot,
        logger,
        "cargo",
        [
            "tauri",
            "build",
            "--config",
            "src-tauri/tauri.release.conf.json",
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

    logger.log("Release artifacts copied to ./releases:");
    for (const artifactPath of copiedArtifacts) {
        logger.log(` - ${path.relative(activeRepositoryRoot, artifactPath)}`);
    }

    return copiedArtifacts;
}

export function runReleaseLocalizationPreflight(repositoryRoot, options = {}) {
    const environment = options.environment || process.env;
    const logger = options.logger || console;
    const localizationDir = path.join(
        repositoryRoot,
        "src-tauri",
        "localizations",
    );
    const strictTranslations = !isTruthyEnvironmentFlag(
        environment[allowLocalizationScaffoldEnvName],
    );

    logger.log("\n>> validate release localization sources");
    validateReleaseLocalizationSources(localizationDir);

    logger.log("\n>> freeze release localization increment");
    if (!strictTranslations) {
        logger.log(
            `localization scaffold override enabled via ${allowLocalizationScaffoldEnvName}; release freeze may scaffold missing non-English working keys from English.`,
        );
    }
    const freezeResult = freezeLocalizationWorkingSet(localizationDir, {
        strictTranslations,
    });

    if (freezeResult.nothingToFreeze) {
        logger.log(
            "en/working.json has no messages; continuing without a new release increment.",
        );
    } else {
        logger.log(
            `frozen ${freezeResult.workingKeyCount} localization keys into ${freezeResult.nextIncrementFileName} for ${freezeResult.officialLocaleCodes.length} official locale(s).`,
        );
    }

    logger.log("\n>> revalidate release localization sources");
    validateReleaseLocalizationSources(localizationDir);

    return {
        freezeResult,
        strictTranslations,
    };
}

function validateReleaseLocalizationSources(localizationDir) {
    const result = syncLocalizationPacks(localizationDir, { checkOnly: true });
    if (result.hasDrift) {
        throw new Error(
            "release localization sources drift from en/origin.json; run npm run localization:sync before packaging",
        );
    }

    return result;
}

function isTruthyEnvironmentFlag(value) {
    if (typeof value !== "string") {
        return false;
    }

    const normalized = value.trim().toLowerCase();
    return normalized === "1" || normalized === "true" || normalized === "yes";
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
                `${baseName}.${versionSegment}.setup.${extension}`;
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

export function normalizeVersionForArtifact(version) {
    const normalizedVersion = normalizeVersionForTag(version);
    const normalized = normalizedVersion
        .replace(/\./g, "_")
        .replace(/[^A-Za-z0-9_-]/g, "_")
        .replace(/_+/g, "_");

    if (!normalized) {
        throw new Error("Artifact version is empty after normalization.");
    }

    return normalized;
}

export function normalizeVersionForTag(version) {
    const normalizedVersion = normalizeVersionCore(version);
    return `v${normalizedVersion}`;
}

function normalizeVersionCore(version) {
    if (!version) {
        throw new Error("Artifact version is missing.");
    }

    const normalized = version.trim().replace(/^v/iu, "");

    if (!normalized) {
        throw new Error("Artifact version is empty after normalization.");
    }

    return normalized;
}

function runCommand(repositoryRoot, logger, command, args, description) {
    logger.log(`\n>> ${description}`);
    const result = spawnSync(command, args, {
        cwd: repositoryRoot,
        stdio: "inherit",
        shell: false,
    });

    if (result.status !== 0) {
        throw new Error(`Command failed: ${command} ${args.join(" ")}`);
    }
}

if (isExecutedAsScript(import.meta.url, process.argv[1])) {
    try {
        runReleaseInstaller();
    } catch (error) {
        console.error(error instanceof Error ? error.message : String(error));
        process.exit(1);
    }
}

function isExecutedAsScript(moduleUrl, executedPath) {
    if (typeof executedPath !== "string" || executedPath.length === 0) {
        return false;
    }

    return path.resolve(executedPath) === fileURLToPath(moduleUrl);
}