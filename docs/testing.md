# 測試

所有 Rust 指令都透過 `scripts/cargo-capped.sh` 執行。這支腳本會用 `systemd-run --user --scope` 限制記憶體（預設 3 GiB，可用 `CARGO_MEM_MAX` 調整），並設定 `CARGO_BUILD_JOBS=2`，避免 rustc 耗盡主機記憶體。

## 準備相依服務

```sh
printf 'POSTGRES_PASSWORD=devpassword123\nRUNNER_TOKEN=dev-runner-token-0123456789abcdef\n' > .env.dev
docker compose --env-file .env.dev -f docker-compose.yml -f docker-compose.dev.yml up -d --wait db runner
# 結果：PostgreSQL 在 127.0.0.1:55432，judge-runner 在 127.0.0.1:17070
```

測試預設會連到上面這兩個服務，也可以用環境變數覆寫：`TEST_DATABASE_URL`、`RUNNER_URL`、`RUNNER_TOKEN`。

## 後端

```sh
scripts/cargo-capped.sh test --workspace     # 全部
scripts/cargo-capped.sh clippy --workspace --all-targets
(cd backend && cargo fmt --all --check)
```

| 測試 | 內容 | 需要 |
|---|---|---|
| `diagnostics` 單元測試 | lexer、模板、GCC JSON 與 ld parser | — |
| `diagnostics/tests/fixtures.rs` | 54 個以**真實 GCC 12.2** 產生的案例：首要規則、行號、證據、連鎖標記，以及反例 | — |
| `diagnostics/tests/catalog_render.rs` | 文案完整性與驗證、依概念切換措辭、壞掉的線上文案會退回內建版本、自訂規則、隱藏測資不外洩 | — |
| `ccal-core` | 局部補寫的組裝與繞過檢查、輸出比對與差異、匯入格式驗證（pointer）、有效設定的優先順序 | — |
| `ccal-api/tests/runner.rs` | 真實沙箱：AC、CE（JSON 診斷）、連結錯誤、segfault、SIGFPE、非 0 回傳、TLE（區分 CPU 與牆上時間）、MLE、OLE、沒有網路、不能 fork、上限截斷、錯誤的 token | runner |
| `ccal-api/tests/api.rs` | 權限矩陣、隱藏測資保護、真實判題的各種結果與卡片、revision 追溯、題目集解耦、切換題目集時草稿與進行中的工作、局部補寫無法繞過、匯入的冪等與全有全無、發布前驗證、提交的冪等、線上文案與組別設定、帳號與 session、個人偏好設定、OpenAPI 與 `docs/openapi.json` 一致 | PostgreSQL + runner |

API 測試會為每個測試建立一個新的資料庫（`ccal_test_<時間>_<亂數>`），一小時以上的舊測試資料庫會自動刪除。

更新 OpenAPI 文件：`backend/target/debug/cyculcal openapi > docs/openapi.json`。

重新產生診斷 fixtures（例如更換編譯器之後）：`scripts/gen-fixtures.sh`。

## 前端

見 [frontend.md](frontend.md)。指令包括 `pnpm build`、`pnpm lint`、`pnpm test`（vitest）與 Playwright。

## 端對端驗收（乾淨環境）

```sh
scripts/create-capped-builder.sh
scripts/e2e.sh            # 或 E2E_KEEP=1 scripts/e2e.sh 保留環境
```

這支腳本會：

1. 以獨立的 compose 專案（`cyculcal-e2e`、port 18181、全新的 volume）從零建置並啟動整套系統。
2. 只透過文件中描述的方式操作：
   - CLI 建立擁有者
   - 用擁有者 session 建立 token
   - `examples/load-demo-content.sh` 與 `examples/agent-demo.sh`
   - 以學生身分呼叫 API

逐項檢查需求中的 10 項驗收條件，並額外檢查：

- 停掉 runner 後，提交顯示 system_error；重新啟動 runner 後可以重新評測。
- 評測途中重啟 app 後，工作會被取回並完成，而且只有一筆結果。
