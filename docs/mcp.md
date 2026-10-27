# MCP 伺服器

`{PUBLIC_URL}/mcp` 是一個遠端 MCP（Model Context Protocol）伺服器，讓 Claude、ChatGPT 等 Agent 直接操作管理功能：出題、驗證、發布、匯入、管理學生與教學文案。雲端 Agent 用 OAuth 連線，由擁有者在網站上授權；本機 Agent 也可以直接用 API token。

管理介面在「管理 → API 與 MCP」：啟用開關、伺服器網址、各家 Agent 的連線方式、已連線的應用（可中斷連線）與 API token。

## 啟用

1. 設定環境變數 `PUBLIC_URL`，也就是網站對外的網址，例如 `https://cal.example.edu`。它是 OAuth 的 issuer，必須和使用者實際連線的網址相同（包含 https）。設定後重新啟動。
2. 在「管理 → API 與 MCP」打開 MCP 開關（存在全站設定 `mcp_enabled`，不需重啟）。關閉時，所有 OAuth 端點與 `/mcp` 立即回 404，既有連線也一併失效。

## 連線

| Agent | 方式 |
|---|---|
| Claude（claude.ai／桌面版） | 設定 → Connectors → Add custom connector，貼上 `{PUBLIC_URL}/mcp`，按 Connect 後到網站授權 |
| ChatGPT | 設定 → Apps → 開啟 Developer mode → Create app，貼上網址，驗證選 OAuth |
| Claude Code | `claude mcp add --transport http ccal {PUBLIC_URL}/mcp`（第一次使用時在瀏覽器授權），或加上 `--header "Authorization: Bearer ccal_…"` 使用 API token |
| 其他 | 任何支援 Streamable HTTP 與 MCP 授權規範的客戶端 |

授權頁會顯示應用程式名稱、**授權後返回的網域**與它要求的權限。擁有者可以縮減權限再按「允許」。重新授權同一個應用會取代舊的連線。

## 工具

- **對應 REST API 的工具**：每個 API token 可以呼叫的管理端點都是一個工具，名稱是 OpenAPI 的 `operationId`（例如 `create_problem`、`update_problem`、`publish_problem`、`import`）。參數是路徑與查詢參數，加上 `body`（請求本文，schema 與 OpenAPI 相同）；`Idempotency-Key`、`If-Match` 標頭對應 `idempotency_key`、`if_match`。工具清單在伺服器啟動時由 OpenAPI 產生，所以 API 改了，MCP 自動跟著改。
- **流程工具**：
  - `save_and_validate_problem`：新建或修改題目，接著用真實評測跑參考解答，`publish: true` 時驗證通過就發布。
  - `import_bundle`：先 dry run，`apply: true` 且沒有錯誤才套用（同一份文件重送不會重複建立）。
  - `publish_set_and_activate`：發布題目集並設為目前啟用。
- **資源**：`ccal://docs/api-guide`、`ccal://docs/import-format`、`ccal://concepts`。

Agent 只看得到連線權限內的工具。API 的錯誤（problem+json，含 `errors[].pointer`）會原樣回傳並標記 `isError`。單次結果超過 256 KiB 會截斷，請用 `limit` 或篩選條件縮小範圍。

所有操作都經過與網頁相同的 handler：權限、驗證、冪等與稽核紀錄都相同。稽核紀錄的「透過」欄會顯示連線的應用名稱。

## 通訊協定細節

- 傳輸：Streamable HTTP，無狀態；只有 `POST /mcp`，回應一律是 `application/json`（不使用 SSE 與 session）。支援協定版本 `2025-11-25`、`2025-06-18`、`2025-03-26`。
- 驗證：`Authorization: Bearer`，接受 OAuth access token（`ccalo_`）或 API token（`ccal_`）。缺少或無效時回 `401`，`WWW-Authenticate` 指向 `/.well-known/oauth-protected-resource/mcp`。
- OAuth 端點：

| 端點 | 規範 |
|---|---|
| `GET /.well-known/oauth-protected-resource[/mcp]` | RFC 9728 |
| `GET /.well-known/oauth-authorization-server`（另有 `openid-configuration`） | RFC 8414 |
| `POST /oauth/register` | RFC 7591 動態註冊（每個 IP 每小時 30 次；7 天內沒有完成授權的會被清除） |
| `GET /oauth/authorize` | 授權碼 + PKCE S256（必填）、RFC 8707 `resource`、RFC 9207 `iss` |
| `POST /oauth/token` | `authorization_code`、`refresh_token`；client 驗證可用 `none`、`client_secret_post`、`client_secret_basic`、`private_key_jwt`（RFC 7523，ChatGPT 使用） |
| `POST /oauth/revoke` | RFC 7009（撤銷任一 token 即中斷整個連線） |

`client_id` 是 https 網址時，視為 Client ID Metadata Document：第一次使用時抓取並快取一天。文件宣告 `private_key_jwt` 時（例如 ChatGPT），token 請求必須附上以文件中 `jwks`／`jwks_uri` 的金鑰簽署的 `client_assertion`；伺服器驗證簽章、`iss`＝`sub`＝client_id、`aud`、到期時間，並拒絕重複使用的 `jti`。

安全設計見 [security.md](security.md)。
