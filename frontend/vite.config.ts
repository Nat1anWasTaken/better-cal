import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': path.resolve(import.meta.dirname, './src') } },
  // The main chunk carries React, Radix and Markdown; the editor and admin are split out.
  // The lazily loaded editor chunk is Monaco's core (~4 MB), which cannot be split further.
  build: { chunkSizeWarningLimit: 4500 },
  server: {
    port: 5173,
    proxy: { '/api': { target: process.env.API_TARGET ?? 'http://localhost:18080', changeOrigin: false } },
  },
})
