import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
    base: "./",
    clearScreen: false,
    plugins: [react()],
    preview: {
        host: "127.0.0.1",
        port: 4173,
        strictPort: true,
    },
    server: {
        host: "127.0.0.1",
        port: 1420,
        strictPort: true,
        watch: {
            ignored: ["**/src-tauri/**"],
        },
    },
});