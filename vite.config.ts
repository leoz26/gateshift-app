import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 走 1420 端口；在非本机开发时用 TAURI_DEV_HOST 开启 HMR 透传。
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // Tauri 干活时别清屏，方便看构建输出。
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // 别去监听 Rust 侧，免得触发前端热重载。
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
  },
});
