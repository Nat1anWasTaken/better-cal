import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: '@', replacement: path.resolve(import.meta.dirname, './src') },
      // monaco-vim imports the pre-0.55 `monaco-editor/esm/vs/...` paths, which the current
      // export map no longer exposes; map them onto the same modules src/lib/monaco.ts uses
      // so there is only one Monaco instance.
      { find: /^monaco-editor\/esm\/vs\/(.*)$/, replacement: 'monaco-editor/$1' },
    ],
  },
  // The main chunk carries React, Radix and Markdown; the editor and admin are split out.
  // The lazily loaded editor chunk is Monaco's core (~4 MB), which cannot be split further.
  build: { chunkSizeWarningLimit: 4500 },
  server: {
    port: 5173,
    proxy: { '/api': { target: process.env.API_TARGET ?? 'http://localhost:18080', changeOrigin: false } },
  },
})
