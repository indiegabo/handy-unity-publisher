import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
    plugins: [react()],
    test: {
        css: true,
        environment: "jsdom",
        include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
        exclude: ["e2e/**"],
        setupFiles: ["./src/test/setup.ts"],
    },
});