import assert from "node:assert/strict";
import {
    mkdirSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { syncLocalizationPacks } from "./sync-localization-packs.mjs";

test("syncLocalizationPacks rejects divergent frozen increment heads", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { recursive: true, force: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: { "app.hello": "Hello" },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: { "app.hello": "Hola" },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {},
    });
    writeLocaleDocument(localizationDir, "en", "increment-1.json", {
        messages: { "app.release": "Released" },
    });

    assert.throws(
        () => syncLocalizationPacks(localizationDir, { checkOnly: true }),
        /official locale es has frozen head 0, expected 1/u,
    );
});

test("syncLocalizationPacks rejects non-English working keys outside English working set", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { recursive: true, force: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: { "app.hello": "Hello" },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: { "app.hello": "Hola" },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: { "app.new": "New copy" },
    });
    writeLocaleDocument(localizationDir, "es", "working.json", {
        messages: { "app.untracked": "No permitido" },
    });

    assert.throws(
        () => syncLocalizationPacks(localizationDir, { checkOnly: true }),
        /es\/working.json contains keys outside en\/working.json/u,
    );
});

test("syncLocalizationPacks backfills non-English origin drift in sync mode", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { recursive: true, force: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
            "app.new": "New copy",
        },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {},
    });

    const result = syncLocalizationPacks(localizationDir, { checkOnly: false });

    assert.equal(result.hasDrift, true);
    assert.deepEqual(readJson(path.join(localizationDir, "es", "origin.json")), {
        messages: {
            "app.hello": "Hola",
            "app.new": "New copy",
        },
    });
});

function createTemporaryLocalizationDir() {
    return mkdtempSync(path.join(os.tmpdir(), "hgp-sync-localizations-"));
}

function writeLocaleDocument(localizationDir, localeCode, fileName, document) {
    const localeDir = path.join(localizationDir, localeCode);
    mkdirSync(localeDir, { recursive: true });
    writeFileSync(
        path.join(localeDir, fileName),
        `${JSON.stringify(document, null, 4)}\n`,
        "utf8",
    );
}

function readJson(filePath) {
    return JSON.parse(readFileSync(filePath, "utf8"));
}