import { spawn } from "node:child_process";
import { Socket } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const desktopApplicationDirectory = resolve(
    repositoryRoot,
    "apps",
    "desktop",
);
const desktopUiDirectory = resolve(desktopApplicationDirectory, "ui");
const desktopUiHost = "127.0.0.1";
const desktopUiPort = 1420;
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

const existingServerState = await inspectDesktopUiServer();
let childProcess;

if (existingServerState === "expected") {
    console.log(
        `Reusing the existing HUP desktop UI dev server on http://${desktopUiHost}:${desktopUiPort}.`,
    );
} else if (existingServerState === "unexpected") {
    throw new Error(
        `Port ${desktopUiPort} is already in use by a service that is not the HUP desktop UI dev server.`,
    );
} else {
    childProcess = spawn(npmCommand, ["run", "dev"], {
        cwd: desktopUiDirectory,
        env: process.env,
        shell: process.platform === "win32",
        stdio: "inherit",
    });

    await Promise.race([
        waitForPort({
            host: desktopUiHost,
            port: desktopUiPort,
            timeoutMs: 30000,
        }),
        waitForChildFailure(childProcess),
    ]);

    const startedServerState = await inspectDesktopUiServer();

    if (startedServerState !== "expected") {
        stopChildProcess(childProcess);
        throw new Error(
            `Expected the HUP desktop UI dev server on ${desktopUiHost}:${desktopUiPort}, but the response did not match the Vite workspace server.`,
        );
    }
}

await waitUntilStopped(childProcess);

async function inspectDesktopUiServer() {
    const isOpen = await tryConnect(desktopUiHost, desktopUiPort);

    if (!isOpen) {
        return "missing";
    }

    try {
        const response = await fetch(`http://${desktopUiHost}:${desktopUiPort}`, {
            signal: AbortSignal.timeout(2000),
        });

        if (!response.ok) {
            return "unexpected";
        }

        const body = await response.text();

        if (body.includes("/@vite/client") && body.includes("/src/main.tsx")) {
            return "expected";
        }

        return "unexpected";
    } catch {
        return "unexpected";
    }
}

function waitForChildFailure(childProcess) {
    return new Promise((resolvePromise, rejectPromise) => {
        childProcess.once("error", (error) => {
            rejectPromise(
                new Error("Failed to launch the desktop UI development server.", {
                    cause: error,
                }),
            );
        });

        childProcess.once("exit", (code, signal) => {
            const exitDetails = signal
                ? `signal ${signal}`
                : `exit code ${code ?? 0}`;

            rejectPromise(
                new Error(
                    `The desktop UI development server exited before becoming ready with ${exitDetails}.`,
                ),
            );
        });
    });
}

function waitUntilStopped(childProcess) {
    return new Promise((resolvePromise, rejectPromise) => {
        let settled = false;
        const keepAliveTimer = childProcess
            ? undefined
            : setInterval(() => {
                // Keep the wrapper process alive while Tauri owns an already-running dev server.
            }, 60_000);

        const settle = (callback) => {
            if (settled) {
                return;
            }

            settled = true;
            if (keepAliveTimer) {
                clearInterval(keepAliveTimer);
            }
            process.off("SIGINT", handleSignal);
            process.off("SIGTERM", handleSignal);
            callback();
        };

        const handleSignal = () => {
            stopChildProcess(childProcess);
            settle(resolvePromise);
        };

        process.on("SIGINT", handleSignal);
        process.on("SIGTERM", handleSignal);

        if (!childProcess) {
            return;
        }

        childProcess.once("error", (error) => {
            settle(() => {
                rejectPromise(
                    new Error("The desktop UI development server failed after startup.", {
                        cause: error,
                    }),
                );
            });
        });

        childProcess.once("exit", (code, signal) => {
            if (signal || code === 0) {
                settle(resolvePromise);
                return;
            }

            settle(() => {
                rejectPromise(
                    new Error(
                        `The desktop UI development server exited unexpectedly with code ${code}.`,
                    ),
                );
            });
        });
    });
}

function stopChildProcess(childProcess) {
    if (!childProcess || childProcess.killed) {
        return;
    }

    childProcess.kill();
}

async function waitForPort({ host, port, timeoutMs }) {
    const timeoutAt = Date.now() + timeoutMs;

    while (Date.now() < timeoutAt) {
        const isOpen = await tryConnect(host, port);

        if (isOpen) {
            return;
        }

        await new Promise((resolvePromise) => setTimeout(resolvePromise, 250));
    }

    throw new Error(`Timed out waiting for ${host}:${port}`);
}

function tryConnect(host, port) {
    return new Promise((resolvePromise) => {
        const socket = new Socket();

        socket.once("connect", () => {
            socket.destroy();
            resolvePromise(true);
        });

        socket.once("error", () => {
            socket.destroy();
            resolvePromise(false);
        });

        socket.setTimeout(1000, () => {
            socket.destroy();
            resolvePromise(false);
        });

        socket.connect(port, host);
    });
}