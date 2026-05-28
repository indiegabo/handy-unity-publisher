import {
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultLocalizationDir = path.resolve(
    scriptDir,
    "../src-tauri/localizations",
);
const englishLocaleCode = "en";
const originFileName = "origin.json";
const workingFileName = "working.json";
const incrementPrefix = "increment-";
const nonEnglishMessageStrategyLocalizedOrEnglish = "localized-or-english";
const nonEnglishMessageStrategyEmpty = "empty";
const cliArguments = new Set(process.argv.slice(2));

export function freezeLocalizationWorkingSet(localizationDir, options = {}) {
    const dryRun = options.dryRun === true;
    const strictTranslations = options.strictTranslations === true;
    const nonEnglishMessageStrategy =
        options.nonEnglishMessageStrategy === nonEnglishMessageStrategyEmpty
            ? nonEnglishMessageStrategyEmpty
            : nonEnglishMessageStrategyLocalizedOrEnglish;

    if (
        strictTranslations
        && nonEnglishMessageStrategy === nonEnglishMessageStrategyEmpty
    ) {
        throw new Error(
            "strict translation mode cannot run with empty non-English increment scaffolding",
        );
    }

    const officialLocaleCodes = readOfficialLocaleCodes(localizationDir);
    const chainState = resolveAlignedIncrementChainState(
        localizationDir,
        officialLocaleCodes,
    );

    const englishLocaleDir = path.join(localizationDir, englishLocaleCode);
    const englishWorkingPath = path.join(englishLocaleDir, workingFileName);
    const englishWorkingDocument = readLocaleOverlayDocument(englishWorkingPath);
    const workingKeys = Object.keys(englishWorkingDocument.messages);

    if (workingKeys.length === 0) {
        return {
            dryRun,
            nextIncrementFileName: `${incrementPrefix}${chainState.nextIncrementNumber}.json`,
            nothingToFreeze: true,
            officialLocaleCodes,
            updates: [],
            workingKeyCount: 0,
        };
    }

    const nextIncrementFileName =
        `${incrementPrefix}${chainState.nextIncrementNumber}.json`;
    const updates = [];

    for (const localeCode of officialLocaleCodes) {
        const localeDir = path.join(localizationDir, localeCode);
        const localeWorkingPath = path.join(localeDir, workingFileName);
        const localeWorkingDocument = existsSync(localeWorkingPath)
            ? readLocaleOverlayDocument(localeWorkingPath)
            : null;

        validateLocaleTranslationCoverage(
            localeCode,
            localeWorkingDocument,
            workingKeys,
            strictTranslations,
        );

        const incrementDocument = buildIncrementDocument(
            localeCode,
            englishWorkingDocument,
            localeWorkingDocument,
            workingKeys,
            nonEnglishMessageStrategy,
        );
        const incrementPath = path.join(localeDir, nextIncrementFileName);

        if (existsSync(incrementPath)) {
            throw new Error(
                `increment file already exists and will not be overwritten: ${incrementPath}`,
            );
        }

        updates.push({
            kind: "write",
            path: incrementPath,
            content: `${JSON.stringify(incrementDocument, null, 4)}\n`,
        });

        if (localeCode === englishLocaleCode) {
            updates.push({
                kind: "write",
                path: localeWorkingPath,
                content: `${JSON.stringify({ messages: {} }, null, 4)}\n`,
            });
        } else if (existsSync(localeWorkingPath)) {
            updates.push({
                kind: "delete",
                path: localeWorkingPath,
            });
        }
    }

    if (!dryRun) {
        applyUpdates(updates);
    }

    return {
        dryRun,
        nextIncrementFileName,
        nothingToFreeze: false,
        officialLocaleCodes,
        updates,
        workingKeyCount: workingKeys.length,
    };
}

async function runCli() {
    const result = freezeLocalizationWorkingSet(defaultLocalizationDir, {
        dryRun: cliArguments.has("--dry-run"),
        strictTranslations: cliArguments.has("--strict-translations"),
    });

    if (result.nothingToFreeze) {
        console.log("en/working.json has no messages; nothing to freeze.");
        return;
    }

    if (result.dryRun) {
        console.log(`dry-run: next increment is ${result.nextIncrementFileName}`);
        for (const update of result.updates) {
            console.log(
                `${update.kind}: ${path.relative(defaultLocalizationDir, update.path)}`,
            );
        }
        return;
    }

    console.log(
        `frozen ${result.workingKeyCount} localization keys into ${result.nextIncrementFileName} for ${result.officialLocaleCodes.length} official locale(s).`,
    );
}

function readOfficialLocaleCodes(localizationDir) {
    const officialLocaleCodes = readdirSync(localizationDir, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name)
        .sort((left, right) => left.localeCompare(right));

    if (!officialLocaleCodes.includes(englishLocaleCode)) {
        throw new Error(`official locale ${englishLocaleCode} is required`);
    }

    return officialLocaleCodes;
}

function resolveAlignedIncrementChainState(localizationDir, officialLocaleCodes) {
    const chainHeads = officialLocaleCodes.map((localeCode) => {
        const localeDir = path.join(localizationDir, localeCode);
        const originPath = path.join(localeDir, originFileName);
        if (!existsSync(originPath)) {
            throw new Error(
                `official locale ${localeCode} is missing ${originFileName}`,
            );
        }

        const incrementNumbers = readdirSync(localeDir)
            .map((fileName) => parseIncrementNumber(fileName))
            .filter((value) => Number.isInteger(value))
            .sort((left, right) => left - right);

        assertContiguousIncrementChain(localeCode, incrementNumbers);

        return {
            localeCode,
            head: incrementNumbers.at(-1) ?? 0,
        };
    });

    const expectedHead = chainHeads.at(0)?.head ?? 0;
    const divergentHeads = chainHeads.filter((entry) => entry.head !== expectedHead);
    if (divergentHeads.length > 0) {
        const headSummary = chainHeads
            .map((entry) => `${entry.localeCode}:${entry.head}`)
            .join(", ");
        throw new Error(
            `official locale increment heads must stay aligned before freeze: ${headSummary}`,
        );
    }

    return {
        chainHeads,
        nextIncrementNumber: expectedHead + 1,
    };
}

function assertContiguousIncrementChain(localeCode, incrementNumbers) {
    for (let index = 0; index < incrementNumbers.length; index += 1) {
        const expectedIncrementNumber = index + 1;
        const actualIncrementNumber = incrementNumbers[index];
        if (actualIncrementNumber !== expectedIncrementNumber) {
            throw new Error(
                `official locale ${localeCode} has a non-contiguous frozen chain; expected increment-${expectedIncrementNumber}.json before increment-${actualIncrementNumber}.json`,
            );
        }
    }
}

function applyUpdates(updates) {
    for (const update of updates) {
        if (update.kind === "write") {
            mkdirSync(path.dirname(update.path), { recursive: true });
            writeFileSync(update.path, update.content, "utf8");
            continue;
        }

        rmSync(update.path, { force: true });
    }
}

function parseIncrementNumber(fileName) {
    if (!fileName.startsWith(incrementPrefix) || !fileName.endsWith(".json")) {
        return null;
    }

    const numericSegment = fileName.slice(
        incrementPrefix.length,
        -".json".length,
    );
    const parsed = Number.parseInt(numericSegment, 10);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

function validateLocaleTranslationCoverage(
    localeCode,
    localeWorkingDocument,
    workingKeys,
    strictTranslations,
) {
    if (!strictTranslations || localeCode === englishLocaleCode) {
        return;
    }

    const missingKeys = workingKeys.filter(
        (key) => typeof localeWorkingDocument?.messages[key] !== "string",
    );
    if (missingKeys.length === 0) {
        return;
    }

    throw new Error(
        `official locale ${localeCode} is missing translations for the current working set: ${missingKeys.join(", ")}`,
    );
}

function buildIncrementDocument(
    localeCode,
    englishWorkingDocument,
    localeWorkingDocument,
    workingKeys,
    nonEnglishMessageStrategy,
) {
    const localizedMessages = Object.fromEntries(
        workingKeys.map((key) => [
            key,
            localeCode === englishLocaleCode
                ? englishWorkingDocument.messages[key]
                : nonEnglishMessageStrategy === nonEnglishMessageStrategyEmpty
                    ? ""
                    : typeof localeWorkingDocument?.messages[key] === "string"
                        ? localeWorkingDocument.messages[key]
                        : englishWorkingDocument.messages[key],
        ]),
    );

    const incrementDocument = {
        messages: localizedMessages,
    };

    const localizedDisplayName = localeWorkingDocument?.display_name?.trim();
    const localizedNativeName = localeWorkingDocument?.native_name?.trim();
    if (localizedDisplayName) {
        incrementDocument.display_name = localizedDisplayName;
    }
    if (localizedNativeName) {
        incrementDocument.native_name = localizedNativeName;
    }

    return incrementDocument;
}

function readLocaleOverlayDocument(filePath) {
    const rawContent = readFileSync(filePath, "utf8");
    const parsed = JSON.parse(rawContent);

    if (!isRecord(parsed) || !isRecord(parsed.messages)) {
        throw new Error(`locale overlay ${filePath} does not expose a messages object`);
    }

    return {
        display_name:
            typeof parsed.display_name === "string" ? parsed.display_name : undefined,
        native_name:
            typeof parsed.native_name === "string" ? parsed.native_name : undefined,
        messages: Object.fromEntries(
            Object.entries(parsed.messages).filter(
                ([key, value]) => typeof key === "string" && typeof value === "string",
            ),
        ),
    };
}

function isRecord(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

if (isExecutedAsScript(import.meta.url, process.argv[1])) {
    runCli().catch((error) => {
        console.error(error instanceof Error ? error.message : String(error));
        process.exit(1);
    });
}

function isExecutedAsScript(moduleUrl, executedPath) {
    if (typeof executedPath !== "string" || executedPath.length === 0) {
        return false;
    }

    return path.resolve(executedPath) === fileURLToPath(moduleUrl);
}