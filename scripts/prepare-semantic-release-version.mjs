import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const nextVersion = process.argv[2];

if (!nextVersion || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/u.test(nextVersion)) {
    throw new Error("Expected the next semantic version as the first argument.");
}

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const cargoManifestPath = resolve(repositoryRoot, "Cargo.toml");
const cargoManifest = readFileSync(cargoManifestPath, "utf8");
const updatedCargoManifest = cargoManifest.replace(
    /(\[workspace\.package\][\s\S]*?^version\s*=\s*")([^"]+)("\s*$)/mu,
    `$1${nextVersion}$3`,
);

if (updatedCargoManifest === cargoManifest) {
    throw new Error("Could not update [workspace.package].version in Cargo.toml.");
}

writeFileSync(cargoManifestPath, updatedCargoManifest);

const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";
const syncResult = spawnSync(npmCommand, ["run", "version:sync"], {
    cwd: repositoryRoot,
    stdio: "inherit",
});

if (syncResult.status !== 0) {
    throw new Error("npm run version:sync failed during semantic release preparation.");
}