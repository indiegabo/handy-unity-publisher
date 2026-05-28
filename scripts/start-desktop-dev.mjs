import { execFileSync, spawn } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { delimiter, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const desktopApplicationDirectory = repositoryRoot;
const desktopDebugBinaryPath = resolve(
    repositoryRoot,
    "tmp",
    "cargo-targets",
    "default",
    "debug",
    "HGP.exe",
);
const runtimeDebugBinaryPath = resolve(
    repositoryRoot,
    "tmp",
    "cargo-targets",
    "default",
    "debug",
    "hgp-runtime.exe",
);
const tauriCommand = resolve(
    repositoryRoot,
    "node_modules",
    ".bin",
    process.platform === "win32" ? "tauri.cmd" : "tauri",
);
const prepareButlerSidecarScript = resolve(
    repositoryRoot,
    "scripts",
    "prepare-butler-sidecar.mjs",
);
const forwardedArguments = process.argv.slice(2);

ensureButlerSidecarPrepared();

const childProcess =
    process.platform === "win32"
        ? spawnWindowsTauriProcess()
        : spawn(tauriCommand, ["dev", ...forwardedArguments], {
            cwd: desktopApplicationDirectory,
            env: process.env,
            stdio: "inherit",
        });

childProcess.on("error", (error) => {
    console.error("Failed to launch the Tauri development command.", error);
    process.exit(1);
});

childProcess.on("exit", (code, signal) => {
    if (signal) {
        process.kill(process.pid, signal);
        return;
    }

    process.exit(code ?? 0);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => {
        stopChildProcess(childProcess);
    });
}

function spawnWindowsTauriProcess() {
    const vsDevCommand = resolveVsDevCommand();

    stopStaleWindowsDevelopmentProcesses();

    if (vsDevCommand) {
        assertMsvcBuildToolsAvailable(vsDevCommand);

        const forwardedCommandArguments = forwardedArguments
            .map(quoteForWindowsCmd)
            .join(" ");
        const tauriDevCommand = [
            `call ${quoteForWindowsCmd(normalizeWindowsPath(tauriCommand))}`,
            "dev",
            forwardedCommandArguments,
        ]
            .filter(Boolean)
            .join(" ");
        const commandLine = [
            `call ${quoteForWindowsCmd(normalizeWindowsPath(vsDevCommand))} -arch=amd64`,
            `cd /d ${quoteForWindowsCmd(normalizeWindowsPath(desktopApplicationDirectory))}`,
            tauriDevCommand,
        ].join(" && ");

        return spawn(commandLine, {
            cwd: desktopApplicationDirectory,
            env: buildWindowsCommandEnvironment(process.env),
            shell: "cmd.exe",
            windowsHide: true,
            stdio: "inherit",
        });
    }

    return spawn(tauriCommand, ["dev", ...forwardedArguments], {
        cwd: desktopApplicationDirectory,
        env: buildWindowsCommandEnvironment(process.env),
        shell: true,
        windowsHide: true,
        stdio: "inherit",
    });
}

function resolveVsDevCommand() {
    const candidatePaths = [
        process.env.VSDEVCMD_PATH,
        "C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\18\\Professional\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\18\\Enterprise\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\18\\BuildTools\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\17\\Community\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\17\\Professional\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\17\\Enterprise\\Common7\\Tools\\VsDevCmd.bat",
        "C:\\Program Files\\Microsoft Visual Studio\\17\\BuildTools\\Common7\\Tools\\VsDevCmd.bat",
    ].filter(Boolean);

    return candidatePaths.find((candidatePath) => existsSync(candidatePath));
}

function ensureButlerSidecarPrepared() {
    execFileSync(process.execPath, [prepareButlerSidecarScript], {
        cwd: repositoryRoot,
        env: process.env,
        stdio: "inherit",
    });
}

function quoteForWindowsCmd(argument) {
    if (argument.length === 0) {
        return '""';
    }

    if (!/[\s"]/u.test(argument)) {
        return argument;
    }

    return `"${argument.replace(/"/g, '""')}"`;
}

function normalizeWindowsPath(path) {
    return path.replaceAll("/", "\\");
}

function stopChildProcess(childProcess) {
    if (!childProcess || childProcess.killed) {
        return;
    }

    childProcess.kill();
}

function assertMsvcBuildToolsAvailable(vsDevCommand) {
    const toolsDirectory = resolveMsvcToolsDirectory(vsDevCommand);

    if (toolsDirectory) {
        return;
    }

    console.error(
        "The detected Visual Studio installation does not include the MSVC C++ build tools. Install the Desktop development with C++ workload before running `npm start`.",
    );
    process.exit(1);
}

function resolveMsvcToolsDirectory(vsDevCommand) {
    const visualStudioDirectory = resolve(dirname(vsDevCommand), "..", "..");
    const msvcRootDirectory = resolve(visualStudioDirectory, "VC", "Tools", "MSVC");

    if (!existsSync(msvcRootDirectory)) {
        return undefined;
    }

    const candidateDirectories = readdirSync(msvcRootDirectory, {
        withFileTypes: true,
    })
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name)
        .sort((left, right) => right.localeCompare(left, undefined, {
            numeric: true,
            sensitivity: "base",
        }));

    for (const candidateDirectory of candidateDirectories) {
        const toolDirectory = resolve(
            msvcRootDirectory,
            candidateDirectory,
            "bin",
            "Hostx64",
            "x64",
        );

        if (
            existsSync(resolve(toolDirectory, "cl.exe"))
            && existsSync(resolve(toolDirectory, "link.exe"))
        ) {
            return toolDirectory;
        }
    }

    return undefined;
}

function buildWindowsCommandEnvironment(environment) {
    const nextEnvironment = { ...environment };
    const pathKey = Object.keys(nextEnvironment).find(
        (key) => key.toUpperCase() === "PATH",
    );

    if (!pathKey) {
        return nextEnvironment;
    }

    nextEnvironment[pathKey] = nextEnvironment[pathKey]
        .split(delimiter)
        .filter((entry) => !isUnixToolchainPath(entry))
        .join(delimiter);

    return nextEnvironment;
}

function stopStaleWindowsDevelopmentProcesses() {
    const targetPaths = [desktopDebugBinaryPath, runtimeDebugBinaryPath]
        .map(normalizeWindowsPath)
        .map(escapePowerShellStringLiteral);

    const command = [
        "$ErrorActionPreference = 'Stop'",
        `$targetPaths = @('${targetPaths.join("','")}')`,
        "$processes = Get-CimInstance Win32_Process | Where-Object {",
        "    $_.ExecutablePath -and $targetPaths -contains $_.ExecutablePath",
        "}",
        "if ($processes) {",
        "    $processes | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }",
        "}",
    ].join("; ");

    try {
        execFileSync("powershell.exe", ["-NoProfile", "-Command", command], {
            stdio: "ignore",
        });
    } catch (error) {
        console.warn(
            "Failed to stop stale desktop development processes before launching Tauri.",
            error,
        );
    }
}

function isUnixToolchainPath(pathEntry) {
    if (!pathEntry) {
        return false;
    }

    const normalizedEntry = pathEntry.replaceAll("/", "\\").toLowerCase();

    return (
        normalizedEntry.includes("\\git\\usr\\bin")
        || normalizedEntry.includes("\\git\\mingw64\\bin")
        || normalizedEntry.includes("\\msys64\\usr\\bin")
        || normalizedEntry.includes("\\msys64\\mingw64\\bin")
        || normalizedEntry === "\\usr\\bin"
        || normalizedEntry === "\\bin"
        || normalizedEntry === "\\mingw64\\bin"
    );
}

function escapePowerShellStringLiteral(value) {
    return value.replaceAll("'", "''");
}
