/// <reference types="node" />

import { readdirSync, readFileSync, statSync } from "fs";
import { extname, join, relative } from "path";
import { describe, expect, it } from "vitest";

const SRC_ROOT = normalizePath(join("src"));
const OVERLAY_MANAGER_FILE = normalizePath(
    join(SRC_ROOT, "components", "OverlayManager.tsx"),
);
const ALLOWED_PORTAL_FILES = new Set([OVERLAY_MANAGER_FILE]);

const SOURCE_FILE_EXTENSIONS = new Set([".ts", ".tsx"]);
const PORTAL_USAGE_PATTERN =
    /\bcreatePortal\b|ReactDOM\.createPortal|from\s+["']react-dom["']/;

describe("Overlay architecture", () => {
    it("keeps portal creation centralized in OverlayManager", () => {
        const sourceFiles = collectSourceFiles(SRC_ROOT);
        const violations = sourceFiles
            .filter((filePath) => !ALLOWED_PORTAL_FILES.has(filePath))
            .filter((filePath) => PORTAL_USAGE_PATTERN.test(readFileSync(filePath, "utf8")))
            .map((filePath) => normalizePath(relative(SRC_ROOT, filePath)));

        expect(violations).toEqual([]);
    });
});

function collectSourceFiles(rootPath: string): string[] {
    const files: string[] = [];
    const queue: string[] = [rootPath];

    while (queue.length > 0) {
        const directory = queue.pop();
        if (!directory) {
            continue;
        }

        for (const entry of readdirSync(directory)) {
            const entryPath = join(directory, entry);
            const stats = statSync(entryPath);

            if (stats.isDirectory()) {
                queue.push(entryPath);
                continue;
            }

            if (!stats.isFile()) {
                continue;
            }

            if (!SOURCE_FILE_EXTENSIONS.has(extname(entryPath))) {
                continue;
            }

            if (entryPath.includes(".test.")) {
                continue;
            }

            files.push(normalizePath(entryPath));
        }
    }

    return files;
}

function normalizePath(pathValue: string) {
    return pathValue.split("\\").join("/");
}
