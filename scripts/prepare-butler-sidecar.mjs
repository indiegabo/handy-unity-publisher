import { execFileSync } from "node:child_process";
import {
    chmodSync,
    copyFileSync,
    createWriteStream,
    existsSync,
    mkdirSync,
    readdirSync,
    rmSync,
} from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";

const BUTLER_DOWNLOAD_VERSION = "LATEST";

const TARGETS = {
    "x86_64-pc-windows-msvc": {
        archiveChannel: "windows-amd64",
        executableName: "butler.exe",
        outputName: "hgp-butler-x86_64-pc-windows-msvc.exe",
    },
    "aarch64-pc-windows-msvc": {
        archiveChannel: "windows-arm64",
        executableName: "butler.exe",
        outputName: "hgp-butler-aarch64-pc-windows-msvc.exe",
    },
    "x86_64-unknown-linux-gnu": {
        archiveChannel: "linux-amd64",
        executableName: "butler",
        outputName: "hgp-butler-x86_64-unknown-linux-gnu",
    },
    "aarch64-unknown-linux-gnu": {
        archiveChannel: "linux-arm64",
        executableName: "butler",
        outputName: "hgp-butler-aarch64-unknown-linux-gnu",
    },
    "x86_64-apple-darwin": {
        archiveChannel: "darwin-amd64",
        executableName: "butler",
        outputName: "hgp-butler-x86_64-apple-darwin",
    },
    "aarch64-apple-darwin": {
        archiveChannel: "darwin-arm64",
        executableName: "butler",
        outputName: "hgp-butler-aarch64-apple-darwin",
    },
};

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const sidecarOutputDirectory = resolve(
    repositoryRoot,
    "apps",
    "desktop",
    "src-tauri",
    "bin",
);

const argumentsMap = parseArguments(process.argv.slice(2));
const targetTriple = argumentsMap.target ?? detectHostTargetTriple();
const target = TARGETS[targetTriple];

if (!target) {
    console.error(
        `Unsupported Butler sidecar target \"${targetTriple}\". Add an explicit mapping before using this platform.`,
    );
    process.exit(1);
}

const outputPath = resolve(sidecarOutputDirectory, target.outputName);

if (!argumentsMap.force && existsSync(outputPath)) {
    console.log(`Butler sidecar already present at ${outputPath}`);
    process.exit(0);
}

await ensureButlerSidecar(target, outputPath);

async function ensureButlerSidecar(target, outputPath) {
    mkdirSync(sidecarOutputDirectory, { recursive: true });

    const temporaryRoot = await mkdtemp(join(tmpdir(), "hgp-butler-sidecar-"));
    const archivePath = resolve(temporaryRoot, `${target.outputName}.zip`);
    const extractionRoot = resolve(temporaryRoot, "extract");

    try {
        const downloadUrl = [
            "https://broth.itch.zone",
            "butler",
            target.archiveChannel,
            BUTLER_DOWNLOAD_VERSION,
            "archive",
            "default",
        ].join("/");

        console.log(`Downloading Butler sidecar from ${downloadUrl}`);
        await downloadFile(downloadUrl, archivePath);

        mkdirSync(extractionRoot, { recursive: true });
        extractArchive(archivePath, extractionRoot);

        const sourceExecutablePath = findFileRecursively(
            extractionRoot,
            target.executableName,
        );

        if (!sourceExecutablePath) {
            throw new Error(
                `Butler archive for ${target.outputName} did not include ${target.executableName}.`,
            );
        }

        copyFileSync(sourceExecutablePath, outputPath);
        if (process.platform !== "win32") {
            chmodSync(outputPath, 0o755);
        }

        console.log(`Prepared Butler sidecar at ${outputPath}`);
    } finally {
        rmSync(temporaryRoot, { force: true, recursive: true });
    }
}

function parseArguments(argv) {
    const parsed = {};

    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index];

        if (argument === "--force") {
            parsed.force = true;
            continue;
        }

        if (argument === "--target") {
            parsed.target = argv[index + 1];
            index += 1;
            continue;
        }
    }

    return parsed;
}

function detectHostTargetTriple() {
    if (process.platform === "win32" && process.arch === "x64") {
        return "x86_64-pc-windows-msvc";
    }

    if (process.platform === "win32" && process.arch === "arm64") {
        return "aarch64-pc-windows-msvc";
    }

    if (process.platform === "linux" && process.arch === "x64") {
        return "x86_64-unknown-linux-gnu";
    }

    if (process.platform === "linux" && process.arch === "arm64") {
        return "aarch64-unknown-linux-gnu";
    }

    if (process.platform === "darwin" && process.arch === "x64") {
        return "x86_64-apple-darwin";
    }

    if (process.platform === "darwin" && process.arch === "arm64") {
        return "aarch64-apple-darwin";
    }

    throw new Error(
        `Unsupported host platform ${process.platform}/${process.arch} for Butler sidecar preparation.`,
    );
}

async function downloadFile(url, destinationPath) {
    const response = await fetch(url);
    if (!response.ok || !response.body) {
        throw new Error(
            `Failed to download Butler sidecar: ${response.status} ${response.statusText}`,
        );
    }

    await pipeline(response.body, createWriteStream(destinationPath));
}

function extractArchive(archivePath, destinationPath) {
    if (process.platform === "win32") {
        execFileSync(
            "powershell.exe",
            [
                "-NoProfile",
                "-Command",
                "Expand-Archive",
                "-LiteralPath",
                archivePath,
                "-DestinationPath",
                destinationPath,
                "-Force",
            ],
            { stdio: "inherit" },
        );
        return;
    }

    execFileSync("unzip", ["-oq", archivePath, "-d", destinationPath], {
        stdio: "inherit",
    });
}

function findFileRecursively(rootPath, fileName) {
    const queue = [rootPath];

    while (queue.length > 0) {
        const currentPath = queue.shift();
        const entries = readdirSync(currentPath, { withFileTypes: true });

        for (const entry of entries) {
            const entryPath = resolve(currentPath, entry.name);

            if (entry.isDirectory()) {
                queue.push(entryPath);
                continue;
            }

            if (entry.isFile() && entry.name === fileName) {
                return entryPath;
            }
        }
    }

    return undefined;
}