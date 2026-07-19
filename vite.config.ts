/// <reference types="vitest" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Set by `tauri android dev`/`tauri ios dev` so the device can reach the Vite
// dev server over the network instead of localhost.
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
  },
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    // The frontend never imports from src-tauri, so Vite has no reason to
    // watch it — but by default it does, and `target/` (Rust build, tens of
    // thousands of dirs) plus `gen/` (Android/Gradle build output) blow
    // through the inotify watch-limit fast, especially running two Vite
    // instances at once (desktop `tauri dev` + `android:dev`).
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  test: {
    globals: true,
    environment: 'happy-dom',
    setupFiles: ['./src/test-setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json-summary'],
      include: ['src/**/*.ts', 'src/**/*.tsx'],
      exclude: [
        'src/**/*.test.ts',
        'src/**/*.test.tsx',
        'src/main.tsx',
        'src/test-setup.ts',
        'src/test-utils.ts',
      ],
    },
  },
})
