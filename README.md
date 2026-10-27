<a id="readme-top"></a>

[![License: AGPL-3.0][license-shield]][license-url]

<br />
<div align="center">
  <a href="https://github.com/Nat1anWasTaken/better-cal">
    <img src="frontend/public/favicon.svg" alt="Logo" width="80" height="80">
  </a>

  <h3 align="center">CYCU-CAL</h3>

  <p align="center">
    給程式新手的 C 語言練習與自動評測平台
    <br />
    <a href="https://github.com/Nat1anWasTaken/better-cal/issues">回報問題</a>
    ·
    <a href="https://github.com/Nat1anWasTaken/better-cal/issues">功能建議</a>
  </p>
</div>

<details>
  <summary>目錄</summary>
  <ol>
    <li>
      <a href="#關於專案">關於專案</a>
      <ul>
        <li><a href="#使用技術">使用技術</a></li>
      </ul>
    </li>
    <li>
      <a href="#開始使用">開始使用</a>
      <ul>
        <li><a href="#需求">需求</a></li>
        <li><a href="#安裝">安裝</a></li>
      </ul>
    </li>
    <li><a href="#使用方式">使用方式</a></li>
    <li><a href="#開發">開發</a></li>
    <li><a href="#授權">授權</a></li>
  </ol>
</details>

## 關於專案

CYCU-CAL 是程式讀書會使用的 C 語言練習平台，對象是完全沒有程式經驗的大學生。

- **看得懂的錯誤說明**：把 GCC 與評測結果轉成白話的繁體中文說明卡，告訴學生發生什麼事、從哪裡看起、下一步怎麼做。
- **線上管理**：在 dashboard 管理題目、題目集、教學文案、學生與設定，修改後立即生效。
- **安全的評測**：學生程式在 isolate 沙箱中以固定的 GCC 12.2（C17）編譯執行，沒有網路、不能 fork。
- **API 與 MCP**：可以用腳本或 AI Agent 出題、匯入與發布。

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

### 使用技術

* [![Rust][Rust-badge]][Rust-url]
* [![PostgreSQL][Postgres-badge]][Postgres-url]
* [![React][React-badge]][React-url]
* [![Vite][Vite-badge]][Vite-url]
* [![TypeScript][TS-badge]][TS-url]
* [![Tailwind CSS][Tailwind-badge]][Tailwind-url]
* [ioi/isolate](https://github.com/ioi/isolate)

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

## 開始使用

### 需求

* Linux 主機，Docker 24 以上
* cgroup v2（Ubuntu 22.04 以上預設）
* 可以執行 privileged 容器（judge-runner 需要）

### 安裝

1. Clone 專案
   ```sh
   git clone https://github.com/Nat1anWasTaken/better-cal.git
   cd better-cal
   ```
2. 建立 `.env`，填入 `POSTGRES_PASSWORD` 與 `RUNNER_TOKEN`（可用 `openssl rand -base64 32` 產生）
   ```sh
   cp .env.example .env
   ```
3. 建置並啟動（`create-capped-builder.sh` 會限制建置時的記憶體用量，可省略）
   ```sh
   scripts/create-capped-builder.sh
   BUILDX_BUILDER=ccal-capped docker compose up -d --build --wait
   ```
4. 建立擁有者帳號
   ```sh
   docker compose exec app /app/cyculcal create-owner --username owner
   ```
5. 開啟 <http://localhost:8080> 登入

正式環境建議放在反向代理後面，並設定 `COOKIE_SECURE=true`、`PUBLIC_ORIGINS` 與 `PUBLIC_URL`。其他設定請參考 [`.env.example`](.env.example)。在 Dokploy 部署時請使用 **Compose** 模式（Swarm 不支援 privileged）。

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

## 使用方式

登入後在「管理 → 學生」建立學生帳號（可貼上 CSV 批次建立），再到「匯入／匯出」上傳題目。

要載入示範教材，先在「管理 → API tokens」建立 token：

```sh
CCAL_URL=http://localhost:8080 CCAL_TOKEN=ccal_... examples/load-demo-content.sh
```

用 API 出題請參考 [API 指南](docs/api-guide.md) 與 [匯入格式](docs/import-format.md)。執行中的伺服器也提供 `/api/docs` 互動式文件。

### MCP

設定 `PUBLIC_URL` 後，在「管理 → API 與 MCP」啟用 MCP，就可以把 `{PUBLIC_URL}/mcp` 加到 Claude、ChatGPT 或 Claude Code，透過 OAuth 授權後管理題庫。

### 備份

```sh
docker compose exec -T db pg_dump -U cyculcal -Fc cyculcal > backup-$(date +%F).dump
```

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

## 開發

```sh
# PostgreSQL（127.0.0.1:55432）與 runner（127.0.0.1:17070）
printf 'POSTGRES_PASSWORD=devpassword123\nRUNNER_TOKEN=dev-runner-token-0123456789abcdef\n' > .env.dev
docker compose --env-file .env.dev -f docker-compose.yml -f docker-compose.dev.yml up -d --wait db runner

# API
scripts/cargo-capped.sh build -p ccal-api
DATABASE_URL=postgres://cyculcal:devpassword123@127.0.0.1:55432/cyculcal \
RUNNER_URL=http://127.0.0.1:17070 RUNNER_TOKEN=dev-runner-token-0123456789abcdef \
BIND=127.0.0.1:18080 backend/target/debug/cyculcal serve

# 前端（/api 會代理到 18080）
cd frontend && pnpm install && pnpm dev
```

測試：

```sh
scripts/cargo-capped.sh test --workspace
cd frontend && pnpm lint && pnpm test
```

修改 API 後，用 `backend/target/debug/cyculcal openapi > docs/openapi.json` 更新 OpenAPI 文件。更換編譯器後，用 `scripts/gen-fixtures.sh` 重新產生診斷 fixtures。

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

## 授權

以 AGPL-3.0 授權釋出，詳見 [`LICENSE`](LICENSE)。

<p align="right">(<a href="#readme-top">回到頂端</a>)</p>

[license-shield]: https://img.shields.io/badge/license-AGPL--3.0-blue.svg?style=for-the-badge
[license-url]: https://github.com/Nat1anWasTaken/better-cal/blob/main/LICENSE
[Rust-badge]: https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white
[Rust-url]: https://www.rust-lang.org/
[Postgres-badge]: https://img.shields.io/badge/PostgreSQL-4169E1?style=for-the-badge&logo=postgresql&logoColor=white
[Postgres-url]: https://www.postgresql.org/
[React-badge]: https://img.shields.io/badge/React-20232A?style=for-the-badge&logo=react&logoColor=61DAFB
[React-url]: https://react.dev/
[Vite-badge]: https://img.shields.io/badge/Vite-646CFF?style=for-the-badge&logo=vite&logoColor=white
[Vite-url]: https://vite.dev/
[TS-badge]: https://img.shields.io/badge/TypeScript-3178C6?style=for-the-badge&logo=typescript&logoColor=white
[TS-url]: https://www.typescriptlang.org/
[Tailwind-badge]: https://img.shields.io/badge/Tailwind_CSS-06B6D4?style=for-the-badge&logo=tailwindcss&logoColor=white
[Tailwind-url]: https://tailwindcss.com/
