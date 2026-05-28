import {
  existsSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const localizationDir = path.resolve(
  scriptDir,
  "../src-tauri/localizations",
);
const sourceLocaleCode = "en";
const sourceLocaleDocumentName = "origin.json";
const workingDocumentName = "working.json";
const incrementFilePrefix = "increment-";
const checkOnly = process.argv.includes("--check");

export function syncLocalizationPacks(localizationDir, options = {}) {
  const checkOnly = options.checkOnly === true;
  const localeCodes = readLocaleCodes(localizationDir);
  validateIncrementalLocalizationContract(localizationDir, localeCodes);

  const sourceDocument = readLocaleDocument(
    path.join(localizationDir, sourceLocaleCode, sourceLocaleDocumentName),
  );
  const sourceEntries = Object.entries(sourceDocument.messages);
  const sourceKeys = sourceEntries.map(([key]) => key);
  const localeReports = [];
  let hasDrift = false;

  for (const localeCode of localeCodes) {
    if (localeCode === sourceLocaleCode) {
      continue;
    }

    const localePath = path.join(
      localizationDir,
      localeCode,
      sourceLocaleDocumentName,
    );
    const localeDocument = readLocaleDocument(localePath);
    const localeKeys = Object.keys(localeDocument.messages);
    const missingKeys = sourceKeys.filter(
      (key) => !(key in localeDocument.messages),
    );
    const extraKeys = localeKeys.filter(
      (key) => !(key in sourceDocument.messages),
    );

    if (missingKeys.length > 0 || extraKeys.length > 0) {
      hasDrift = true;
    }

    if (!checkOnly) {
      const synchronizedMessages = Object.fromEntries(
        sourceEntries.map(([key, englishValue]) => [
          key,
          typeof localeDocument.messages[key] === "string"
            ? localeDocument.messages[key]
            : englishValue,
        ]),
      );

      const {
        display_name,
        messages: _messages,
        native_name,
        ...rest
      } = localeDocument;
      const synchronizedDocument = {
        display_name,
        native_name,
        ...rest,
        messages: synchronizedMessages,
      };

      writeFileSync(
        localePath,
        `${JSON.stringify(synchronizedDocument, null, 4)}\n`,
        "utf8",
      );
    }

    localeReports.push({
      localeCode,
      missingKeys,
      extraKeys,
      mode: checkOnly ? "checked" : "synced",
    });
  }

  return {
    checkOnly,
    hasDrift,
    localeReports,
  };
}

function runCli() {
  const result = syncLocalizationPacks(localizationDir, { checkOnly });

  for (const report of result.localeReports) {
    reportLocaleDrift(
      report.localeCode,
      report.missingKeys,
      report.extraKeys,
      report.mode,
    );
  }

  if (result.checkOnly && result.hasDrift) {
    console.error(
      `localization packs drift from ${sourceLocaleCode}/${sourceLocaleDocumentName}; run npm run localization:sync`,
    );
    process.exit(1);
  }

  if (result.checkOnly) {
    console.log(
      `all non-English locale packs match ${sourceLocaleCode}/${sourceLocaleDocumentName}`,
    );
  } else {
    console.log(
      `non-English locale packs synchronized from ${sourceLocaleCode}/${sourceLocaleDocumentName}`,
    );
  }
}

function readLocaleCodes(localizationDir) {
  const localeCodes = readdirSync(localizationDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right));

  if (!localeCodes.includes(sourceLocaleCode)) {
    throw new Error(`official locale ${sourceLocaleCode} is required`);
  }

  return localeCodes;
}

function validateIncrementalLocalizationContract(localizationDir, localeCodes) {
  const issues = [];
  const englishWorkingPath = path.join(
    localizationDir,
    sourceLocaleCode,
    workingDocumentName,
  );

  if (!existsSync(englishWorkingPath)) {
    issues.push(
      `${sourceLocaleCode}/${workingDocumentName} is required for the active localization cycle`,
    );
  }

  const englishWorkingDocument = existsSync(englishWorkingPath)
    ? readLocaleDocument(englishWorkingPath)
    : { messages: {} };
  const englishWorkingKeys = Object.keys(englishWorkingDocument.messages);
  const englishIncrementDocuments = readLocaleIncrementDocuments(
    localizationDir,
    sourceLocaleCode,
  );
  const englishIncrementByNumber = new Map(
    englishIncrementDocuments.map((document) => [document.number, document]),
  );
  const expectedIncrementNumbers = englishIncrementDocuments.map(
    (document) => document.number,
  );
  const expectedHead = expectedIncrementNumbers.at(-1) ?? 0;

  for (const localeCode of localeCodes) {
    const localeOriginPath = path.join(
      localizationDir,
      localeCode,
      sourceLocaleDocumentName,
    );
    if (!existsSync(localeOriginPath)) {
      issues.push(`${localeCode}/${sourceLocaleDocumentName} is required`);
      continue;
    }

    const incrementDocuments = readLocaleIncrementDocuments(
      localizationDir,
      localeCode,
    );
    const incrementNumbers = incrementDocuments.map((document) => document.number);
    const head = incrementNumbers.at(-1) ?? 0;
    if (head !== expectedHead) {
      issues.push(
        `official locale ${localeCode} has frozen head ${head}, expected ${expectedHead}`,
      );
    }

    for (const expectedNumber of expectedIncrementNumbers) {
      const localeIncrement = incrementDocuments.find(
        (document) => document.number === expectedNumber,
      );
      if (!localeIncrement) {
        issues.push(
          `${localeCode}/increment-${expectedNumber}.json is required to match the English release chain`,
        );
        continue;
      }

      const englishIncrement = englishIncrementByNumber.get(expectedNumber);
      const { missingKeys, extraKeys } = compareMessageKeySets(
        englishIncrement.document.messages,
        localeIncrement.document.messages,
      );
      if (missingKeys.length > 0 || extraKeys.length > 0) {
        issues.push(
          `${localeCode}/increment-${expectedNumber}.json must preserve the English key set for the same release increment`,
        );
      }
    }

    if (localeCode === sourceLocaleCode) {
      continue;
    }

    const localeWorkingPath = path.join(
      localizationDir,
      localeCode,
      workingDocumentName,
    );
    if (!existsSync(localeWorkingPath)) {
      continue;
    }

    const localeWorkingDocument = readLocaleDocument(localeWorkingPath);
    const extraWorkingKeys = Object.keys(localeWorkingDocument.messages).filter(
      (key) => !englishWorkingKeys.includes(key),
    );
    if (extraWorkingKeys.length > 0) {
      issues.push(
        `${localeCode}/${workingDocumentName} contains keys outside en/${workingDocumentName}: ${extraWorkingKeys.join(", ")}`,
      );
    }
  }

  if (issues.length > 0) {
    throw new Error(issues.join("\n"));
  }
}

function readLocaleIncrementDocuments(localizationDir, localeCode) {
  const localeDirectory = path.join(localizationDir, localeCode);
  const incrementDocuments = readdirSync(localeDirectory)
    .map((fileName) => ({
      fileName,
      number: parseIncrementNumber(fileName),
    }))
    .filter((entry) => Number.isInteger(entry.number))
    .sort((left, right) => left.number - right.number)
    .map((entry) => ({
      number: entry.number,
      document: readLocaleDocument(path.join(localeDirectory, entry.fileName)),
    }));

  for (let index = 0; index < incrementDocuments.length; index += 1) {
    const expectedNumber = index + 1;
    const actualNumber = incrementDocuments[index].number;
    if (actualNumber !== expectedNumber) {
      throw new Error(
        `official locale ${localeCode} has a non-contiguous frozen chain; expected increment-${expectedNumber}.json before increment-${actualNumber}.json`,
      );
    }
  }

  return incrementDocuments;
}

function compareMessageKeySets(sourceMessages, candidateMessages) {
  const sourceKeys = Object.keys(sourceMessages);
  const candidateKeys = Object.keys(candidateMessages);

  return {
    missingKeys: sourceKeys.filter((key) => !(key in candidateMessages)),
    extraKeys: candidateKeys.filter((key) => !(key in sourceMessages)),
  };
}

function parseIncrementNumber(fileName) {
  if (!fileName.startsWith(incrementFilePrefix) || !fileName.endsWith(".json")) {
    return null;
  }

  const numericSegment = fileName.slice(
    incrementFilePrefix.length,
    -".json".length,
  );
  const parsed = Number.parseInt(numericSegment, 10);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

function readLocaleDocument(filePath) {
  const rawContent = readFileSync(filePath, "utf8");
  const parsed = JSON.parse(rawContent);

  if (!isRecord(parsed) || !isRecord(parsed.messages)) {
    throw new Error(`locale file ${filePath} does not expose a messages object`);
  }

  return {
    ...parsed,
    messages: Object.fromEntries(
      Object.entries(parsed.messages).filter(
        ([key, value]) => typeof key === "string" && typeof value === "string",
      ),
    ),
  };
}

function reportLocaleDrift(
  localeCode,
  missingKeys,
  extraKeys,
  mode = "checked",
) {
  const segments = [`${localeCode}/${sourceLocaleDocumentName} ${mode}`];

  if (missingKeys.length > 0) {
    segments.push(`missing ${missingKeys.length}`);
  }

  if (extraKeys.length > 0) {
    segments.push(`extra ${extraKeys.length}`);
  }

  if (missingKeys.length === 0 && extraKeys.length === 0) {
    segments.push("aligned");
  }

  console.log(segments.join(" · "));

  if (missingKeys.length > 0) {
    console.log(`  missing keys: ${missingKeys.slice(0, 10).join(", ")}`);
  }

  if (extraKeys.length > 0) {
    console.log(`  extra keys: ${extraKeys.slice(0, 10).join(", ")}`);
  }
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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