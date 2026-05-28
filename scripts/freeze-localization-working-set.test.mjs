import assert from "node:assert/strict";
import {
    existsSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { freezeLocalizationWorkingSet } from "./freeze-localization-working-set.mjs";

test("freezeLocalizationWorkingSet mirrors the next increment and clears working files", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { force: true, recursive: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        display_name: "English",
        native_name: "English",
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        display_name: "Spanish",
        native_name: "Espanol",
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {
            "app.hello": "Updated hello",
            "app.new": "Brand new",
        },
    });
    writeLocaleDocument(localizationDir, "es", "working.json", {
        messages: {
            "app.hello": "Hola actualizada",
        },
    });

    const result = freezeLocalizationWorkingSet(localizationDir);

    assert.equal(result.nextIncrementFileName, "increment-1.json");
    assert.deepEqual(
        readJson(path.join(localizationDir, "en", "increment-1.json")),
        {
            messages: {
                "app.hello": "Updated hello",
                "app.new": "Brand new",
            },
        },
    );
    assert.deepEqual(
        readJson(path.join(localizationDir, "es", "increment-1.json")),
        {
            messages: {
                "app.hello": "Hola actualizada",
                "app.new": "Brand new",
            },
        },
    );
    assert.deepEqual(
        readJson(path.join(localizationDir, "en", "working.json")),
        { messages: {} },
    );
    assert.equal(
        existsSync(path.join(localizationDir, "es", "working.json")),
        false,
    );
});

test("freezeLocalizationWorkingSet rejects divergent official increment heads", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { force: true, recursive: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(localizationDir, "en", "increment-1.json", {
        messages: {
            "app.hello": "Hello release",
        },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {
            "app.new": "Brand new",
        },
    });

    assert.throws(
        () => freezeLocalizationWorkingSet(localizationDir, { dryRun: true }),
        /official locale increment heads must stay aligned before freeze/u,
    );
});

test("freezeLocalizationWorkingSet enforces translated coverage in strict mode", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { force: true, recursive: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {
            "app.new": "Brand new",
        },
    });

    assert.throws(
        () =>
            freezeLocalizationWorkingSet(localizationDir, {
                dryRun: true,
                strictTranslations: true,
            }),
        /official locale es is missing translations for the current working set/u,
    );
});

test("freezeLocalizationWorkingSet supports empty non-English prerelease increments", (t) => {
    const localizationDir = createTemporaryLocalizationDir();
    t.after(() => {
        rmSync(localizationDir, { force: true, recursive: true });
    });

    writeLocaleDocument(localizationDir, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(localizationDir, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(localizationDir, "en", "working.json", {
        messages: {
            "app.hello": "Updated hello",
            "app.new": "Brand new",
        },
    });

    const result = freezeLocalizationWorkingSet(localizationDir, {
        nonEnglishMessageStrategy: "empty",
    });

    assert.equal(result.nextIncrementFileName, "increment-1.json");
    assert.deepEqual(
        readJson(path.join(localizationDir, "en", "increment-1.json")),
        {
            messages: {
                "app.hello": "Updated hello",
                "app.new": "Brand new",
            },
        },
    );
    assert.deepEqual(
        readJson(path.join(localizationDir, "es", "increment-1.json")),
        {
            messages: {
                "app.hello": "",
                "app.new": "",
            },
        },
    );
    assert.deepEqual(
        readJson(path.join(localizationDir, "en", "working.json")),
        { messages: {} },
    );
});

function createTemporaryLocalizationDir() {
    return mkdtempSync(path.join(os.tmpdir(), "hgp-freeze-localizations-"));
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