import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const defaultCargoTargetDir = resolve(
    repositoryRoot,
    "tmp",
    "cargo-targets",
    "runtime-smoke",
);

const internalFlags = new Set(["--dry-run"]);
const forwardedArguments = process.argv.slice(2);
const dryRun = forwardedArguments.includes("--dry-run");
const runnerArguments = forwardedArguments.filter(
    (argument) => !internalFlags.has(argument),
);

const cargoCommand = resolveCargoCommand();
const cargoTargetDir =
    process.env.CARGO_TARGET_DIR?.trim() || defaultCargoTargetDir;
const cargoArguments = [
    "test",
    "-p",
    "runtime-bin",
    "--test",
    "interrupted_cleanup_e2e",
    "--test",
    "publish_destinations_e2e",
    "--",
    "--nocapture",
    ...runnerArguments,
];

mkdirSync(cargoTargetDir, { recursive: true });

if (dryRun) {
    console.log(`Resolved cargo command: ${cargoCommand}`);
    console.log(`CARGO_TARGET_DIR=${cargoTargetDir}`);
    console.log(`Command: ${formatCommand(cargoCommand, cargoArguments)}`);
    process.exit(0);
}

console.log(
    `Running runtime smoke tests with CARGO_TARGET_DIR=${cargoTargetDir}`,
);

const result = spawnSync(cargoCommand, cargoArguments, {
    cwd: repositoryRoot,
    env: {
        ...process.env,
        CARGO_TARGET_DIR: cargoTargetDir,
    },
    stdio: "inherit",
});

if (result.error) {
    throw result.error;
}

process.exit(result.status ?? 0);

function resolveCargoCommand() {
    const candidates = [];
    const configuredCargo = process.env.CARGO_BIN?.trim();

    if (configuredCargo) {
        candidates.push(configuredCargo);
    }

    candidates.push("cargo");

    if (process.platform === "win32") {
        candidates.push("cargo.exe");
    }

    const cargoHome = join(
        homedir(),
        ".cargo",
        "bin",
        process.platform === "win32" ? "cargo.exe" : "cargo",
    );
    candidates.push(cargoHome);

    if (process.platform === "win32") {
        candidates.push(join(homedir(), ".cargo", "bin", "cargo"));
    }

    for (const candidate of candidates) {
        if (isUsableCargoCommand(candidate)) {
            return candidate;
        }
    }

    throw new Error(
        [
            "runtime smoke could not resolve cargo.",
            "Set CARGO_BIN explicitly or install Rust so cargo is available on PATH.",
        ].join(" "),
    );
}

function isUsableCargoCommand(command) {
    if (looksLikePath(command) && !existsSync(command)) {
        return false;
    }

    const result = spawnSync(command, ["--version"], {
        stdio: "ignore",
        env: process.env,
    });

    return !result.error && result.status === 0;
}

function looksLikePath(value) {
    return value.includes("/") || value.includes("\\") || value.includes(delimiter);
}

function formatCommand(command, argumentsList) {
    return [command, ...argumentsList].map(quoteArgument).join(" ");
}

function quoteArgument(argument) {
    if (!/[\s"]/u.test(argument)) {
        return argument;
    }

    return `"${argument.replaceAll('"', '\\"')}"`;
}