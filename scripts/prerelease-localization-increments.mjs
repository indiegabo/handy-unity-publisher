import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { freezeLocalizationWorkingSet } from "./freeze-localization-working-set.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultLocalizationDir = path.resolve(
    scriptDir,
    "../src-tauri/localizations",
);
const cliArguments = new Set(process.argv.slice(2));

export function generatePrereleaseLocalizationIncrements(localizationDir, options = {}) {
    return freezeLocalizationWorkingSet(localizationDir, {
        dryRun: options.dryRun === true,
        strictTranslations: false,
        nonEnglishMessageStrategy: "empty",
    });
}

function runCli() {
    const result = generatePrereleaseLocalizationIncrements(
        defaultLocalizationDir,
        {
            dryRun: cliArguments.has("--dry-run"),
        },
    );

    if (result.nothingToFreeze) {
        console.log("en/working.json has no messages; nothing to freeze for prerelease.");
        return;
    }

    if (result.dryRun) {
        console.log(
            `dry-run: prerelease increment is ${result.nextIncrementFileName}`,
        );
        for (const update of result.updates) {
            console.log(
                `${update.kind}: ${path.relative(defaultLocalizationDir, update.path)}`,
            );
        }
        return;
    }

    console.log(
        `prerelease generated ${result.nextIncrementFileName} for ${result.officialLocaleCodes.length} official locale(s): English frozen and non-English increment keys initialized with empty values.`,
    );
}

if (isExecutedAsScript(import.meta.url, process.argv[1])) {
    try {
        runCli();
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
