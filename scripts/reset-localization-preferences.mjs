#!/usr/bin/env node
import { join } from "node:path";
import { rmSync } from "node:fs";

const appDataDir = process.env.APPDATA;

if (!appDataDir) {
  console.error("missing APPDATA environment variable");
  process.exit(1);
}

const settingsPath = join(
  appDataDir,
  "HandyGamesPublisher",
  "localization-settings.json",
);

try {
  rmSync(settingsPath, { force: true });
  console.log(`removed ${settingsPath}`);
} catch (error) {
  console.error(`failed to reset localization preferences: ${error.message}`);
  process.exitCode = 1;
}
