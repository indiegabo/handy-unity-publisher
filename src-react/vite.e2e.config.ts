import { resolve } from "node:path";
import { defineConfig, mergeConfig } from "vite";

import baseConfig from "./vite.config";

export default mergeConfig(
    baseConfig,
    defineConfig({
        resolve: {
            alias: {
                "@tauri-apps/api/core": resolve(
                    __dirname,
                    "src/test/e2e/mocks/tauriCore.ts",
                ),
                "@tauri-apps/api/event": resolve(
                    __dirname,
                    "src/test/e2e/mocks/tauriEvent.ts",
                ),
                "@tauri-apps/api/window": resolve(
                    __dirname,
                    "src/test/e2e/mocks/tauriWindow.ts",
                ),
            },
        },
        server: {
            host: "127.0.0.1",
            port: 4174,
            strictPort: true,
        },
    }),
);