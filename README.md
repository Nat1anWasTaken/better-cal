# CYCU-CAL C 語言教學與自動評測平台

CYCU-CAL 程式讀書會使用的 C 語言練習平台，對象是完全沒有程式經驗的大學生。重點有兩個：

- **新手看得懂的錯誤說明**：把真實 GCC 與評測結果轉換成白話的繁體中文卡片，包含「發生什麼事、從哪裡看起、為什麼、下一步」，提示可以一層層展開，並清楚區分「編譯器確認」與「系統推測」。
- **線上管理**：擁有者在 dashboard 管理題目、題目集、教學文案、學生與設定，修改後立即生效，不需要重新部署。

| 部分 | 技術 |
|---|---|
| 後端 | Rust（axum、sqlx、PostgreSQL 17） |
| 前端 | React 19、Vite、TypeScript、Tailwind CSS v4、shadcn/ui、Monaco Editor |
| 評測 | 自建 judge-runner：**GCC 12.2（C17）+ ioi/isolate 2.7**（cgroup v2）。詳見 [docs/toolchain.md](docs/toolchain.md)，其中說明為什麼不用 Judge0 |

## 快速開始（自架）

```sh
cp .env.example .env                       # 填入 POSTGRES_PASSWORD 與 RUNNER_TOKEN
scripts/create-capped-builder.sh           # 建置時限制記憶體（建議）
BUILDX_BUILDER=ccal-capped docker compose up -d --build --wait
docker compose exec app /app/cyculcal create-owner --username owner
```

開啟 <http://localhost:8080> 並登入。如果要載入示範教材（自建教材），在「管理 → API tokens」建立 token 後執行：

```sh
CCAL_URL=http://localhost:8080 CCAL_TOKEN=ccal_... examples/load-demo-content.sh
```

需要 Linux 主機、cgroup v2，並允許 privileged 容器（runner 使用）。在 Dokploy 上部署請用 **Compose** 模式，見 [docs/operations.md](docs/operations.md)。

## 開發

```sh
# 相依服務：PostgreSQL（127.0.0.1:55432）與 runner（127.0.0.1:17070）
printf 'POSTGRES_PASSWORD=devpassword123\nRUNNER_TOKEN=dev-runner-token-0123456789abcdef\n' > .env.dev
docker compose --env-file .env.dev -f docker-compose.yml -f docker-compose.dev.yml up -d --wait db runner

# API（使用記憶體上限的 cargo）
scripts/cargo-capped.sh build -p ccal-api
DATABASE_URL=postgres://cyculcal:devpassword123@127.0.0.1:55432/cyculcal \
RUNNER_URL=http://127.0.0.1:17070 RUNNER_TOKEN=dev-runner-token-0123456789abcdef \
BIND=127.0.0.1:18080 backend/target/debug/cyculcal serve

# 前端（Vite 會把 /api 代理到 18080）
cd frontend && pnpm install && pnpm dev
```

## 文件

| 文件 | 內容 |
|---|---|
| [docs/api-guide.md](docs/api-guide.md) | 給 Agent／腳本的 API 指南：token、錯誤格式、冪等、發布流程 |
| [docs/mcp.md](docs/mcp.md) | MCP 伺服器：讓 Claude、ChatGPT 等 Agent 以 OAuth 連線管理題庫 |
| [docs/openapi.json](docs/openapi.json) | OpenAPI 3（執行中的伺服器也提供 `/api/v1/openapi.json` 與 `/api/docs`） |
| [docs/import-format.md](docs/import-format.md) | JSON 匯入／匯出格式 `cycu-cal.import/v1` |
| [docs/diagnostics.md](docs/diagnostics.md) | 診斷轉譯層的架構、規則、測試，以及如何線上編輯文案 |
| [docs/toolchain.md](docs/toolchain.md) | 固定的編譯器、C 標準、編譯選項與執行限制 |
| [docs/operations.md](docs/operations.md) | 部署、Dokploy、故障恢復、備份、環境變數 |
| [docs/security.md](docs/security.md) | 沙箱、隱藏測資、身分驗證、CSRF、權限 |
| [docs/testing.md](docs/testing.md) | 各層測試與端對端驗收（`scripts/e2e.sh`） |
| [docs/frontend.md](docs/frontend.md) | 前端結構與設計決策 |

## 目錄

```
backend/crates/diagnostics   診斷轉譯層（規則、真實 GCC fixtures、教學文案種子）
backend/crates/core          局部補寫、輸出比對、匯入格式、設定的優先順序
backend/crates/api           HTTP API、判題佇列、CLI（cyculcal）
backend/crates/runner        judge-runner（isolate 封裝）
backend/migrations           資料庫遷移
frontend/                    學生介面與管理 dashboard
content/demo/                示範題目（CYCU-CAL 自建教材）
examples/                    agent-demo.sh、load-demo-content.sh
deploy/                      Dockerfile、isolate 設定
scripts/                     cargo-capped.sh、gen-fixtures.sh、e2e.sh
```
