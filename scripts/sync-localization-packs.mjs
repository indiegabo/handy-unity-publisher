import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const localizationDir = path.resolve(
  scriptDir,
  "../apps/desktop/src-tauri/localizations",
);
const sourceLocaleCode = "en";
const sourceLocaleFileName = `${sourceLocaleCode}.json`;
const checkOnly = process.argv.includes("--check");

const sourceDocument = readLocaleDocument(
  path.join(localizationDir, sourceLocaleFileName),
);
const sourceEntries = Object.entries(sourceDocument.messages);
const sourceKeys = sourceEntries.map(([key]) => key);
const localeFileNames = readdirSync(localizationDir)
  .filter((entry) => entry.endsWith(".json"))
  .sort((left, right) => left.localeCompare(right));

let hasDrift = false;

for (const localeFileName of localeFileNames) {
  if (localeFileName === sourceLocaleFileName) {
    continue;
  }

  const localePath = path.join(localizationDir, localeFileName);
  const localeDocument = readLocaleDocument(localePath);
  const localeKeys = Object.keys(localeDocument.messages);
  const missingKeys = sourceKeys.filter(
    (key) => !(key in localeDocument.messages),
  );
  const extraKeys = localeKeys.filter((key) => !(key in sourceDocument.messages));

  if (missingKeys.length > 0 || extraKeys.length > 0) {
    hasDrift = true;
  }

  if (checkOnly) {
    reportLocaleDrift(localeFileName, missingKeys, extraKeys);
    continue;
  }

  const synchronizedMessages = Object.fromEntries(
    sourceEntries.map(([key, englishValue]) => [
      key,
      typeof localeDocument.messages[key] === "string"
        ? localeDocument.messages[key]
        : englishValue,
    ]),
  );

  const { display_name, messages: _messages, native_name, ...rest } = localeDocument;
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

  reportLocaleDrift(localeFileName, missingKeys, extraKeys, "synced");
}

if (checkOnly && hasDrift) {
  console.error(
    `localization packs drift from ${sourceLocaleFileName}; run npm run localization:sync`,
  );
  process.exit(1);
}

if (checkOnly) {
  console.log(`all non-English locale packs match ${sourceLocaleFileName}`);
} else {
  console.log(`non-English locale packs synchronized from ${sourceLocaleFileName}`);
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
  localeFileName,
  missingKeys,
  extraKeys,
  mode = "checked",
) {
  const segments = [`${localeFileName} ${mode}`];

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