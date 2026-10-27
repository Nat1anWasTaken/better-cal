# 安全設計

## 執行學生程式

- 每次評測都在獨立的 isolate 沙箱中執行，有獨立的 PID、mount、IPC 與 network 命名空間，並以專屬的 uid 執行。
- 資源限制：
  - cgroup v2 記憶體上限，swap 為 0。
  - CPU 時間與牆上時間。
  - 輸出檔大小（`--fsize`）。
  - `--processes=1`，不能 fork。
- 沒有網路：沒有加上 `--share-net`，所以沙箱只有空的 network namespace。測試 `sandbox_has_no_network_and_cannot_fork` 會驗證這一點。
- 編譯同樣在沙箱內執行，有自己的時間與記憶體限制（10 s CPU、20 s 牆上時間、512 MiB、64 行程）。
- runner 容器本身：
  - privileged，這是 isolate 建立 cgroup 與命名空間的需要，Judge0 也是同樣的做法。
  - 只接在 `internal` Docker 網路上，不對外開 port。
  - 用共用的 bearer token 驗證，以常數時間比較。
  - 有整體記憶體上限與 pids 上限。
  - 沙箱根目錄放在 tmpfs。
- **只放在 Docker 裡不等於已經隔離**：真正的隔離來自 isolate 的命名空間與 cgroup，Docker 只是封裝與部署方式。runner 會用自己的上限截斷每個請求的限制，請求端不能指定編譯器或編譯選項。

## 隱藏測資

- 預期輸出永遠不會傳給 runner，答案比對在 API 內進行。
- 學生端所有提交內容都由同一個函式組成（`judge/view.rs` 的 `view::build`，Audience::Student）。隱藏測資只回傳「隱藏測資 #n」、判定結果、時間與記憶體；不會回傳輸入、預期輸出、實際輸出、stderr 或差異。
- 診斷分析只拿到隱藏測資的狀態，不會拿到它的輸出或 stderr，所以教學卡片上不可能出現隱藏內容。
- 題目頁面不包含隱藏測資與參考解答。
- 這條規則寫死在程式中，不能透過設定關閉。
- 整合測試 `hidden_tests_never_reach_students` 與 e2e 都會在回應中搜尋隱藏測資特有的值。

## 身分驗證

- 密碼使用 RustCrypto `argon2` 0.5 的 Argon2id 預設參數，最少 8 個字元。帳號不存在時也會執行一次雜湊比對，避免從回應時間判斷帳號是否存在。登入有頻率限制。
- Session：
  - 256-bit 隨機 token，放在 `HttpOnly; SameSite=Lax` cookie，可設定 `Secure`。資料庫只存 SHA-256。
  - 有效期會在使用時延長（預設 14 天）。
  - 修改密碼時會清除該使用者的其他 session；停用帳號或重設密碼時會清除所有 session。
- 為什麼不用 `tower-sessions-sqlx-store`：它停在 sqlx 0.8 / tower-sessions-core 0.14（2025-01），而且我們需要能依使用者撤銷所有 session。所以採用 OWASP 建議的不透明 token 設計，只使用標準的密碼學元件（OS 亂數、SHA-256），沒有自行設計任何密碼學機制。
- CSRF 防護：
  - 以 cookie 驗證的非 GET 請求必須帶 `X-CSRF-Token`，值必須等於 session 綁定的隨機值。
  - 若有設定 `PUBLIC_ORIGINS`，也會檢查 `Origin`。
  - 另有 SameSite=Lax 作為額外防護。
  - Bearer token 請求不使用 cookie，所以不需要 CSRF token。
- API token：
  - 格式為 `ccal_` 加上 256-bit 亂數，只存 SHA-256。
  - 有 scope 限制，可設定到期，可撤銷。
  - token 不能建立 token、不能重設密碼、不能建立擁有者。
- 沒有預設帳號，也不開放註冊。第一個擁有者只能用伺服器上的 CLI 建立。

## 權限

所有 handler 都透過同一個 `Principal` extractor 取得身分，再由 `require(scope)` 或 `require_owner_session()` 檢查。三種身分的規則：

| 身分 | 權限 |
|---|---|
| 學生 | 只能讀取已發布、且在自己可見題目集中的題目；只能存取自己的提交與草稿。別人的提交一律回 404，避免透露它是否存在 |
| 擁有者 session | 全部權限 |
| API token | 擁有者身分加上 scope 限制 |

整合測試 `permissions_are_enforced_for_every_kind_of_caller` 涵蓋這些規則。

## 其他

- **Markdown**：前端使用 `rehype-sanitize`，不允許 raw HTML。
- **安全標頭**：
  - 全站：`Content-Security-Policy`（`default-src 'self'`）、`X-Frame-Options: DENY`、`nosniff`、`Referrer-Policy`。
  - API 回應：`Cache-Control: no-store`。
- **自訂診斷規則的正規表達式**：使用 Rust `regex`（線性時間、有大小上限），沒有 ReDoS 風險。
- **稽核**：所有管理操作都寫入 `audit_log`（誰、透過哪個 token、做了什麼）。
