#!/usr/bin/env node

import { spawn } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectoryPath = path.dirname(fileURLToPath(import.meta.url));
const repositoryRootPath = path.resolve(scriptDirectoryPath, "..");
const iconsDirectoryPath = path.join(
    repositoryRootPath,
    "src-tauri",
    "icons",
);
const appIconSourcePath = path.join(iconsDirectoryPath, "app-icon-source.png");

function runCommand(command, cwd) {
    return new Promise((resolve, reject) => {
        const child = spawn(command, {
            cwd,
            shell: true,
            stdio: "inherit",
        });

        child.on("error", reject);
        child.on("close", (code) => {
            if (code === 0) {
                resolve();
                return;
            }

            reject(
                new Error(
                    `${command} exited with code ${code ?? "unknown"}`,
                ),
            );
        });
    });
}

async function pathExists(targetPath) {
    try {
        await access(targetPath, fsConstants.F_OK);
        return true;
    } catch {
        return false;
    }
}

async function main() {
    if (!(await pathExists(appIconSourcePath))) {
        throw new Error(
            `missing required app icon source: ${appIconSourcePath}`,
        );
    }

    await runCommand(
        `npm exec tauri icon -- "${appIconSourcePath}" -o "${iconsDirectoryPath}"`,
        repositoryRootPath,
    );
}

main().catch((error) => {
    console.error(`[icons] ${error.message}`);
    process.exitCode = 1;
});