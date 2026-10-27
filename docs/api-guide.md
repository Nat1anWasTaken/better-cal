# API 指南（給 Agent 與腳本）

這份文件讓另一個 Agent **只讀文件**就能出題、驗證、匯入及發布，不需要操作瀏覽器。完整的端點與 schema 請見：

- OpenAPI 3：`GET /api/v1/openapi.json`（同一份也在 `docs/openapi.json`，有測試確保與程式一致）
- 互動式文件：`/api/docs`

所有路徑都以 `/api/v1` 開頭。

支援 MCP 的 Agent（Claude、ChatGPT、Claude Code…）也可以直接連 MCP 伺服器 `{網站}/mcp`，工具與這裡的 API 一一對應，見 [mcp.md](mcp.md)。

## 1. 認證

### API token（Agent 用）

由擁有者在「管理 → API tokens」建立，或用擁有者的瀏覽器 session 呼叫 `POST /api/v1/admin/tokens`。token 不能用來建立 token。

```http
Authorization: Bearer ccal_xxxxxxxxxxxxxxxx
```

- secret 只在建立時顯示一次，資料庫只存 SHA-256。
- 可以設定有效天數，也可以隨時撤銷：`DELETE /admin/tokens/{id}`，撤銷後立即失效。
- 權限（scope）採最小權限原則：

| scope | 允許的操作 |
|---|---|
| `problems:read` | 讀取題庫、題目集、匯出、驗證、總覽 |
| `problems:write` | 建立／修改題目、匯入、試跑（test-run） |
| `sets:write` | 建立／修改題目集與題目順序 |
| `publish` | 發布或下架題目與題目集、設定目前啟用的題目集 |
| `submissions:read` | 讀取所有提交（含隱藏測資細節） |
| `content:write` | 編輯教學文案（診斷說明） |
| `students:read` / `students:write` | 查看或管理學生、組別、進度 |
| `settings:write` | 全站設定、稽核紀錄 |

出題 Agent 一般需要 `problems:read problems:write sets:write publish`。

### 瀏覽器 session（學生與擁有者）

`POST /auth/login` 會設定 HttpOnly cookie，並回傳 `csrf_token`。之後所有非 GET 的請求都必須帶 `X-CSRF-Token` 標頭。若有設定 `PUBLIC_ORIGINS`，`Origin` 也必須在允許清單中。

每個使用者的偏好設定（編輯器配色、Vim 模式、字體大小、縮排寬度、自動換行、行號、minimap）放在 `GET /auth/me` 的 `user.preferences`，用 `PUT /auth/preferences` 整份取代；沒有給的欄位會用預設值。API token 不能修改偏好設定。

## 2. 共通慣例

- **識別碼**：資源以 UUID 為主鍵。題目與題目集另有不可修改的 `slug`，路徑參數兩者皆可用，例如 `/admin/problems/a-plus-b`。
- **錯誤**：一律是 RFC 9457 `application/problem+json`：

  ```json
  {
    "type": "/errors/validation_failed", "title": "Unprocessable Entity", "status": 422,
    "code": "validation_failed", "detail": "請求內容有欄位不符合規則",
    "errors": [{"pointer": "/content/limits/time_ms", "code": "out_of_range", "message": "時間限制必須介於 100 與 5000 毫秒之間"}]
  }
  ```

  `errors[].pointer` 是 JSON Pointer，指向請求本文中出問題的欄位。`code` 是穩定的機器可讀代碼。
- **分頁**：清單端點回傳 `{"items": [...], "next_cursor": "..."}`，下一頁傳 `?cursor=...&limit=1..100`。cursor 是不透明字串，不要自行解析。
- **搜尋**：`GET /admin/problems?q=關鍵字&status=draft|published|archived`。
- **HTTP 語意**：建立回 `201`；非同步工作（提交評測）回 `202`；成功但無內容回 `204`；版本衝突回 `412`（`If-Match`）或 `409`；欄位錯誤回 `422`；頻率限制回 `429`；評測服務不可用回 `503`。

## 3. 安全重試（不重複建立）

- `POST /admin/problems`、`POST /admin/problem-sets`、`POST /admin/import`、`POST /submissions` 都支援 `Idempotency-Key` 標頭。
  - 同一個 key 加上相同的本文：回傳第一次的回應，不會再建立一次。
  - 同一個 key 加上不同的本文：回 `409 idempotency_key_reused`。
- 題目與題目集以 `slug` 識別。匯入時內容雜湊相同視為 `unchanged`，不同則建立新的 revision，所以**重送整份匯入文件永遠不會產生重複內容**。
- `PUT /admin/problems/{slug}` 的內容如果沒有變，不會建立新版本（回應中 `revision_created: false`）。

## 4. 題目的生命週期

```
建立（draft, revision 1）→ 修改（revision 2, 3…，都是不可變的快照）
→ validate（用參考解答跑全部測資，經過真實評測）→ publish（學生開始看到指定的 revision）
```

- 學生只會看到 `published_revision`。修改後必須再 publish 才會生效。
- 每筆提交都記錄它評測時使用的 revision，所以舊的提交永遠可以追溯：`GET /admin/problems/{slug}/revisions/{n}`。
- 發布前的檢查：
  - 必須有題敘。
  - 至少 1 筆公開測資（學生按「執行」時用）。
  - 隱藏測資可有可無；沒有的話，「提交」只會跑公開測資。
  - 必須有參考解答，而且要通過全部測資。
  - 評測服務無法使用時回 `503`，不會在未經驗證的情況下發布。
- 題目集必須所有題目都已發布才能發布；只有已發布的題目集可以設為「目前啟用」。
- 匯入的內容一律是**草稿**。發布與啟用都是另外的明確操作。

## 5. 最短流程（curl）

完整可執行的版本見 `examples/agent-demo.sh`。

```sh
API=http://localhost:8080/api/v1
H=(-H "Authorization: Bearer $CCAL_TOKEN" -H 'Content-Type: application/json')

# 1) 建立題目與測資（草稿）
curl -sS "${H[@]}" -H 'Idempotency-Key: create-even-odd' -X POST $API/admin/problems --data '{
  "slug": "even-odd",
  "content": {
    "title": "奇數還是偶數",
    "statement_md": "輸入整數 n，偶數輸出 even，奇數輸出 odd。",
    "mode": "full",
    "concepts": ["io", "conditionals"],
    "reference_solution": "#include <stdio.h>\nint main(void){int n;scanf(\"%d\",&n);puts(n%2==0?\"even\":\"odd\");return 0;}\n",
    "tests": [
      {"visibility": "public", "input": "4\n", "expected_output": "even\n"},
      {"visibility": "hidden", "input": "-3\n", "expected_output": "odd\n"}
    ]
  }
}'

# 2) 驗證（不改變任何東西）
curl -sS "${H[@]}" -X POST $API/admin/problems/even-odd/validate --data '{}'

# 3) 發布題目
curl -sS "${H[@]}" -X POST $API/admin/problems/even-odd/publish --data '{}'

# 4) 建立題目集並排序（problems 可用 slug 或 id）
curl -sS "${H[@]}" -H 'Idempotency-Key: create-week-3' -X POST $API/admin/problem-sets \
  --data '{"slug":"week-3","title":"第 3 週","learned_concepts":["io","conditionals"],"problems":["even-odd","a-plus-b"]}'

# 5) 發布題目集，並設為目前啟用（學生進站預設看到它，不需重啟）
curl -sS "${H[@]}" -X POST $API/admin/problem-sets/week-3/publish
curl -sS "${H[@]}" -X PUT $API/admin/site/active-problem-set --data '{"problem_set":"week-3"}'
```

## 6. 批次匯入

見 [import-format.md](import-format.md)。步驟：

1. `POST /admin/import?dry_run=true`：回報每一項的 `create`／`update`／`unchanged` 與帶 pointer 的錯誤，不會寫入任何資料。
2. `POST /admin/import`（加上 `Idempotency-Key`）：在同一個交易中套用，**全部成功或全部失敗**。
3. 逐題 `POST /admin/problems/{slug}/publish`，再發布題目集、設定啟用。

`GET /admin/export[?problem_set=slug][&published=true]` 匯出的格式與匯入相同，可以直接再匯入（結果全部是 `unchanged`）。

## 7. 題目內容（ProblemContent）重點

| 欄位 | 說明 |
|---|---|
| `mode` | `full`（完整作答）或 `fill_in`（局部補寫） |
| `starter_code` | full 模式的起始程式 |
| `template` | fill_in 模式的範本。可編輯區用 `/*@blank 名稱@*/預設內容/*@end@*/` 標記，其他部分固定 |
| `reference_solution` / `reference_blanks` | 只有擁有者看得到，用來驗證測資 |
| `tests[]` | `{name?, visibility: public\|hidden, input, expected_output}`，最多 50 筆，每筆最多 1 MiB |
| `limits` | `time_ms` 100–5000、`memory_kb` 16384–262144、`output_kb` 1–1024 |
| `compare_mode` | `trim_trailing_whitespace`（預設：忽略每行行尾空白與結尾空行）或 `exact` |
| `concepts` | 題目假設學生已學會的概念，影響說明的措辭。可用值見 `GET /concepts` |

fill_in 題目的學生端只能送出 `blanks`。固定程式碼一律由伺服器用儲存的範本組裝，直接呼叫 API 送 `code` 會被拒絕（`422 /code`）。

## 8. 教學文案、學生與設定

這些都可以透過 API 修改，例如：

- `PUT /admin/catalog/rules/{rule}` → `POST /admin/catalog/preview` → `POST /admin/catalog/publish`
- `POST /admin/cohorts`、`PUT /admin/users/{id}/policy`
- `PUT /admin/settings`

說明見 [diagnostics.md](diagnostics.md) 與 OpenAPI 文件。

## 9. 排行榜（輪次）

排行以「輪」為單位，同時只會有一輪進行中。開始新一輪最方便的方式，是在設為目前啟用時一起開始：

```sh
# 啟用 week-3，結束進行中的一輪並開始新的一輪（title 省略時用題目集名稱）
curl -sS "${H[@]}" -X PUT $API/admin/site/active-problem-set \
  --data '{"problem_set":"week-3","start_round":{"title":"第 3 週挑戰"}}'
```

- `POST /admin/rounds/{id}/freeze`：凍結。學生只看得到凍結當下的排名，之後的提交顯示為「?」；擁有者一直看到即時結果。
- `POST /admin/rounds/{id}/reveal`：揭曉（解除凍結）。
- `POST /admin/rounds/{id}/end`：結束。之後的提交不再計入。
- 學生端：`GET /leaderboard`（進行中的一輪）、`GET /rounds`、`GET /rounds/{id}/leaderboard`。

計分規則（`ccal_core::leaderboard`）：

- 只計算這一輪題目集內的題目，以及這一輪開始到結束之間的「提交」（不含執行、自訂測試）。這一輪之前通過的題目要重新提交才算。管理者認列不計入排行。
- 先比通過題數，再比罰時。罰時 = 每題從這一輪開始到第一次通過的分鐘數 + 通過前每次錯誤提交 20 分鐘。編譯失敗不算錯誤。三者都相同時名次並列。
- 只列出這一輪有提交過的學生。有效設定 `leaderboard: false` 的學生（可在全站、組別或個人設定）看不到排行，也不會被列出。
- 徽章：`first_solve`（首殺：本輪第一位通過該題）、`no_hints`（通過前沒有在本輪展開過該題的提示）。依設定 `auto_reveal_hints` 自動顯示的提示不會被記錄，因此不影響 `no_hints`。之後可能新增其他種類，用戶端遇到未知的 `kind` 應直接忽略或原樣顯示。

## 10. 練習場

`POST /playground/run`：用和評測相同的編譯器與沙箱，編譯並執行任意一支 C 程式一次，回傳輸出和教學卡片。不會建立提交，也不會儲存任何內容。

```sh
curl -sS "${H[@]}" -X POST $API/playground/run \
  --data '{"source":"#include <stdio.h>\nint main(void){int x;scanf(\"%d\",&x);printf(\"%d\\n\",x*2);}","stdin":"21"}'
```

- 回應：`compile_success`、`run`（狀態、輸出、stderr、時間與記憶體；無法編譯時為 null）、`cards`、`auto_reveal_hints`、`limits`。
- 沒有預期輸出，所以只會出現編譯、警告與執行期（崩潰、超時、超過記憶體或輸出上限）的卡片，不會有答案錯誤或隱藏測資的說明。
- `cards[].hints` 已包含這位使用者可以看到的所有提示（依 `max_hint_levels`）。用戶端先顯示前 `auto_reveal_hints` 個，其餘在學生要求時再顯示。
- 限制：程式碼與輸入各最多 64 KiB；使用題目的預設資源限制（1 秒、64 MB、輸出 64 KiB）。每位使用者和「執行」共用 `runs_per_minute` 的次數設定（另外計數），同時只能有一個執行；全站同時執行數由 `PLAYGROUND_CONCURRENCY` 限制。
- 有效設定 `playground: false`（可在全站、組別或個人設定）時回 403，`GET /site` 的 `playground` 也會是 false。擁有者一律可以使用。
