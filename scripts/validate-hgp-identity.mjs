import { readdirSync, readFileSync, statSync } from "node:fs"
import { dirname, extname, join, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const scriptDirectory = dirname(fileURLToPath(import.meta.url))
const repositoryRoot = resolve(scriptDirectory, "..")

const ignoredDirectories = new Set([".git", "node_modules", "tmp"])

const textExtensions = new Set([
    ".css",
    ".html",
    ".js",
    ".json",
    ".md",
    ".mjs",
    ".ps1",
    ".rs",
    ".sh",
    ".sql",
    ".toml",
    ".tsx",
    ".ts",
    ".txt",
    ".yaml",
    ".yml",
])

const ignoredFiles = new Set([
    "planning/hgp-refactor-master-plan.md",
    "scripts/validate-hgp-identity.mjs",
])

const legacyProductShortName = ["H", "U", "P"].join("")
const legacyProductTitle = ["Handy", "Unity", "Publisher"].join(" ")
const legacyRepositoryName = ["handy", "unity", "publisher"].join("-")
const legacyEnvPrefix = ["HANDY", "UNITY", "PUBLISHER"].join("_")
const legacyBuilderEnvPrefix = ["HANDY", "UNITY", "BUILDER"].join("_")
const legacyRuntimeBinary = `${legacyProductShortName.toLowerCase()}-runtime`
const legacyWorkspaceToken = ["Handy", "Unity", "Publisher"].join("")

const forbiddenPatterns = [
    {
        label: legacyProductShortName,
        regex: new RegExp(`\\b${escapeForRegExp(legacyProductShortName)}\\b`, "g"),
    },
    {
        label: legacyProductTitle,
        regex: new RegExp(escapeForRegExp(legacyProductTitle), "g"),
    },
    {
        label: legacyRepositoryName,
        regex: new RegExp(escapeForRegExp(legacyRepositoryName), "g"),
    },
    {
        label: legacyEnvPrefix,
        regex: new RegExp(escapeForRegExp(legacyEnvPrefix), "g"),
    },
    {
        label: legacyBuilderEnvPrefix,
        regex: new RegExp(escapeForRegExp(legacyBuilderEnvPrefix), "g"),
    },
    {
        label: legacyRuntimeBinary,
        regex: new RegExp(`\\b${escapeForRegExp(legacyRuntimeBinary)}\\b`, "g"),
    },
    {
        label: legacyWorkspaceToken,
        regex: new RegExp(escapeForRegExp(legacyWorkspaceToken), "g"),
    },
]

const findings = []

walk(repositoryRoot)

if (findings.length > 0) {
    console.error("Legacy product identity references found:")
    for (const finding of findings) {
        console.error(
            `- ${finding.file}:${finding.line} [${finding.label}] ${finding.text}`,
        )
    }
    process.exit(1)
}

console.log(
    "HGP identity sweep passed. No legacy HUP references found outside intentional historical files.",
)

function walk(directoryPath) {
    for (const entry of readdirSync(directoryPath, { withFileTypes: true })) {
        if (entry.isDirectory()) {
            if (ignoredDirectories.has(entry.name)) {
                continue
            }

            walk(join(directoryPath, entry.name))
            continue
        }

        if (!entry.isFile()) {
            continue
        }

        const absolutePath = join(directoryPath, entry.name)
        const relativePath = relative(repositoryRoot, absolutePath).replaceAll("\\", "/")
        if (ignoredFiles.has(relativePath)) {
            continue
        }

        if (!shouldInspectFile(absolutePath)) {
            continue
        }

        inspectFile(absolutePath, relativePath)
    }
}

function shouldInspectFile(filePath) {
    const extension = extname(filePath).toLowerCase()
    if (textExtensions.has(extension)) {
        return true
    }

    return filePath.endsWith(".gitignore")
}

function inspectFile(absolutePath, relativePath) {
    const stats = statSync(absolutePath)
    if (stats.size > 1024 * 1024) {
        return
    }

    let contents
    try {
        contents = readFileSync(absolutePath, "utf8")
    } catch {
        return
    }

    const lines = contents.split(/\r?\n/)
    for (let index = 0; index < lines.length; index += 1) {
        const line = lines[index]
        for (const pattern of forbiddenPatterns) {
            pattern.regex.lastIndex = 0
            if (!pattern.regex.test(line)) {
                continue
            }

            findings.push({
                file: relativePath,
                line: index + 1,
                label: pattern.label,
                text: line.trim(),
            })
        }
    }
}

function escapeForRegExp(value) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
}