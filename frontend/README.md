# CYCU-CAL web UI

React + Vite frontend for the CYCU-CAL C teaching and judging platform.
See [../docs/frontend.md](../docs/frontend.md) for structure, scripts and design decisions.

```sh
pnpm install
pnpm dev          # http://localhost:5173, proxies /api to API_TARGET (default http://localhost:18080)
pnpm build        # dist/, served by the Rust app via STATIC_DIR
pnpm lint && pnpm test
pnpm e2e          # Playwright against a running stack (BASE_URL, default http://localhost:5173)
```
