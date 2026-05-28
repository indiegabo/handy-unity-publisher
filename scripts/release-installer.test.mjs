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

import {
    normalizeVersionForArtifact,
    normalizeVersionForTag,
    runReleaseLocalizationPreflight,
} from "./release-installer.mjs";

test("normalizeVersionForTag prefixes plain SemVer with v", () => {
    assert.equal(normalizeVersionForTag("0.1.0"), "v0.1.0");
});

test("normalizeVersionForTag keeps a single v prefix", () => {
    assert.equal(normalizeVersionForTag("v0.1.0"), "v0.1.0");
});

test("normalizeVersionForArtifact prefixes and normalizes the version for filenames", () => {
    assert.equal(normalizeVersionForArtifact("0.1.0"), "v0_1_0");
});

test("normalizeVersionForArtifact keeps a single v prefix in filenames", () => {
    assert.equal(normalizeVersionForArtifact("v0.1.0"), "v0_1_0");
});

test("runReleaseLocalizationPreflight freezes the working set before release packaging", (t) => {
    const repositoryRoot = createTemporaryRepositoryRoot();
    t.after(() => {
        rmSync(repositoryRoot, { recursive: true, force: true });
    });

    writeLocaleDocument(repositoryRoot, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(repositoryRoot, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(repositoryRoot, "en", "working.json", {
        messages: {
            "app.release": "Release copy",
        },
    });
    writeLocaleDocument(repositoryRoot, "es", "working.json", {
        messages: {
            "app.release": "Texto de release",
        },
    });

    const result = runReleaseLocalizationPreflight(repositoryRoot, {
        logger: createSilentLogger(),
    });

    assert.equal(result.freezeResult.nothingToFreeze, false);
    assert.equal(result.freezeResult.nextIncrementFileName, "increment-1.json");
    assert.deepEqual(
        readJson(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "en",
                "working.json",
            ),
        ),
        { messages: {} },
    );
    assert.equal(
        existsSync(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "es",
                "working.json",
            ),
        ),
        false,
    );
    assert.deepEqual(
        readJson(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "es",
                "increment-1.json",
            ),
        ),
        {
            messages: {
                "app.release": "Texto de release",
            },
        },
    );
});

test("runReleaseLocalizationPreflight fails before mutation when origin drift exists", (t) => {
    const repositoryRoot = createTemporaryRepositoryRoot();
    t.after(() => {
        rmSync(repositoryRoot, { recursive: true, force: true });
    });

    writeLocaleDocument(repositoryRoot, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
            "app.release": "Release copy",
        },
    });
    writeLocaleDocument(repositoryRoot, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(repositoryRoot, "en", "working.json", {
        messages: {
            "app.release": "Release copy",
        },
    });

    assert.throws(
        () =>
            runReleaseLocalizationPreflight(repositoryRoot, {
                logger: createSilentLogger(),
            }),
        /release localization sources drift from en\/origin.json/u,
    );
    assert.equal(
        existsSync(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "en",
                "increment-1.json",
            ),
        ),
        false,
    );
});

test("runReleaseLocalizationPreflight is strict by default", (t) => {
    const repositoryRoot = createTemporaryRepositoryRoot();
    t.after(() => {
        rmSync(repositoryRoot, { recursive: true, force: true });
    });

    writeLocaleDocument(repositoryRoot, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(repositoryRoot, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(repositoryRoot, "en", "working.json", {
        messages: {
            "app.release": "Release copy",
        },
    });

    assert.throws(
        () =>
            runReleaseLocalizationPreflight(repositoryRoot, {
                logger: createSilentLogger(),
            }),
        /official locale es is missing translations for the current working set/u,
    );
    assert.equal(
        existsSync(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "en",
                "increment-1.json",
            ),
        ),
        false,
    );
});

test("runReleaseLocalizationPreflight allows scaffold only with explicit override", (t) => {
    const repositoryRoot = createTemporaryRepositoryRoot();
    t.after(() => {
        rmSync(repositoryRoot, { recursive: true, force: true });
    });

    writeLocaleDocument(repositoryRoot, "en", "origin.json", {
        messages: {
            "app.hello": "Hello",
        },
    });
    writeLocaleDocument(repositoryRoot, "es", "origin.json", {
        messages: {
            "app.hello": "Hola",
        },
    });
    writeLocaleDocument(repositoryRoot, "en", "working.json", {
        messages: {
            "app.release": "Release copy",
        },
    });

    const result = runReleaseLocalizationPreflight(repositoryRoot, {
        environment: {
            HGP_RELEASE_ALLOW_LOCALIZATION_SCAFFOLD: "1",
        },
        logger: createSilentLogger(),
    });

    assert.equal(result.strictTranslations, false);
    assert.equal(result.freezeResult.nextIncrementFileName, "increment-1.json");
    assert.deepEqual(
        readJson(
            path.join(
                repositoryRoot,
                "src-tauri",
                "localizations",
                "es",
                "increment-1.json",
            ),
        ),
        {
            messages: {
                "app.release": "Release copy",
            },
        },
    );
});

function createTemporaryRepositoryRoot() {
    return mkdtempSync(path.join(os.tmpdir(), "hgp-release-installer-"));
}

function writeLocaleDocument(repositoryRoot, localeCode, fileName, document) {
    const localeDir = path.join(
        repositoryRoot,
        "src-tauri",
        "localizations",
        localeCode,
    );
    mkdirSync(localeDir, { recursive: true });
    writeFileSync(
        path.join(localeDir, fileName),
        `${JSON.stringify(document, null, 4)}\n`,
        "utf8",
    );
}

function createSilentLogger() {
    return {
        log() { },
    };
}

function readJson(filePath) {
    return JSON.parse(readFileSync(filePath, "utf8"));
}