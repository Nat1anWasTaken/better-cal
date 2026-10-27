# 部署與維運

## 元件

| 服務 | 內容 | 網路 |
|---|---|---|
| `app` | Rust API、判題佇列 worker，以及前端 SPA（同一個 process） | `backend` 與 `judge` |
| `runner` | judge-runner：GCC 12.2 + isolate 2.7，privileged | 只在 `judge`（`internal: true`，不能連外，也不對外開 port） |
| `db` | PostgreSQL 17，資料在具名 volume `db-data` | 只在 `backend` |

只有 `app` 對外開放 port（預設 8080）。學生的程式在 isolate 沙箱中執行，每個沙箱都有自己空的網路命名空間，所以完全沒有網路。

## 自架（Docker Compose）

```sh
cp .env.example .env            # 填入 POSTGRES_PASSWORD、RUNNER_TOKEN（openssl rand -base64 32）
scripts/create-capped-builder.sh   # 可選：建立有記憶體上限（3 GiB）的 buildx builder，避免編譯 Rust 時耗盡主機記憶體
BUILDX_BUILDER=ccal-capped docker compose up -d --build --wait
docker compose exec app /app/cyculcal create-owner --username owner   # 互動式輸入密碼
```

開啟 `http://<主機>:8080`，用擁有者帳號登入，接著：

- 在「管理 → 學生」建立學生帳號，可以貼上 CSV 批次建立；系統不開放自行註冊。
- 匯入教材：
  - 在「匯入／匯出」上傳 JSON；或
  - 建立 API token 後執行 `examples/load-demo-content.sh` 載入示範教材。

需求：

- Linux 主機，Docker 24 以上。
- **cgroup v2**（Ubuntu 22.04 以上預設就是）。
- 可以執行 privileged 容器。

`runner` 啟動時會執行 `isolate-check-environment`，有問題會印在容器 log。

### HTTPS 與反向代理

建議放在反向代理（Traefik、Caddy 或 Nginx）後面，並設定：

```
COOKIE_SECURE=true
PUBLIC_ORIGINS=https://judge.example.org
```

## Dokploy

1. 新增 **Compose** 類型的應用程式（Docker Compose），**不要用 Stack / Swarm 模式**。原因是 Swarm service 不支援 `privileged`，而 runner 需要它才能建立 cgroup 與命名空間。
2. 來源指向這個 repo，Compose 檔為 `docker-compose.yml`。
3. 在 Environment 設定 `POSTGRES_PASSWORD`、`RUNNER_TOKEN`、`COOKIE_SECURE=true`、`PUBLIC_ORIGINS=https://你的網域`。
4. Domains：把網域指到 service `app`、port `8080`，由 Dokploy 的 Traefik 處理 HTTPS。如果不需要直接暴露主機 port，可以移除 `ports`。
5. 部署後，在 Dokploy 的終端機（或主機上）執行 `docker compose exec app /app/cyculcal create-owner --username owner`。

建置時編譯 Rust 需要大約 2–3 GiB 記憶體。Dockerfile 已固定 `CARGO_BUILD_JOBS=2`。如果主機記憶體較小，建議在另一台機器建好映像推到 registry，再讓 Dokploy 直接拉映像。

> 實際在 Dokploy 上部署**尚未在本專案中驗證**；以上步驟依 Dokploy 的 Compose 模式整理。

## 不需重啟的線上變更

以下操作都直接寫入資料庫，並立即生效：

- 題目、測資、題目集、發布狀態、目前啟用的題目集
- 教學文案
- 組別與學生設定
- 全站設定

做法：API 在修改時遞增 `site_config.version`，並送出 `NOTIFY ccal_config`。其他 API 行程收到通知（或在每 10 秒的版本檢查中發現變化）後重新載入快取。

## 評測佇列、重試與故障恢復

提交會先寫入 `submissions` 資料表（`status = queued`），之後由 worker 處理：

1. 以 `FOR UPDATE SKIP LOCKED` 認領工作，並取得 30 秒的租約（lease）。執行期間每 10 秒續約一次。
2. 依序進入 `compiling` → `running` 兩個階段（各呼叫 runner 一次），最後寫入結果（`done`）。
3. 寫入結果時，以 `(id, attempts, status)` 作為條件。過期的 worker 不能覆寫新的結果，所以不會重複計分。

| 狀況 | 系統行為 | 學生看到的 |
|---|---|---|
| runner 暫時無法連線或回傳錯誤 | 指數退避重試（4 s、8 s…），最多 `JUDGE_MAX_ATTEMPTS`（預設 3）次 | 「等待評測中」 |
| 重試用盡 | `status = system_error`，記錄 `last_error` | 「評測服務發生問題（不是你的程式造成的）」，以及「重新評測」按鈕 |
| runner 恢復後 | 學生或擁有者按重新評測（`POST /submissions/{id}/rejudge`），仍使用原本的題目 revision | 正常結果 |
| app 在評測途中當機或重啟 | 租約在 30 秒內到期，重啟後的 worker（或其他 app 行程）重新認領，`attempts + 1` | 稍微延遲後得到結果 |
| PostgreSQL 重啟 | API 自動重連。佇列在資料庫裡，不會遺失 | — |

- 前端輪詢有上限。超過約 90 秒仍未完成時，會明確告訴學生是服務問題。
- `GET /api/v1/system/status` 回報 runner 健康狀態與佇列長度，學生頁面會依此顯示橫幅。擁有者的總覽頁面也會顯示最近 24 小時的 `system_error` 數量。
- 查看失敗原因：「管理 → 提交紀錄」的明細頁會顯示 `last_error` 與 `attempts`，或執行 `docker compose logs runner app`。

### 恢復 runner

```sh
docker compose ps runner          # 健康狀態
docker compose logs --tail=100 runner
docker compose restart runner
```

如果 log 出現 `cgroup v2 is required`，代表主機不是 cgroup v2，或容器沒有 privileged 權限。

## 流量與資源限制

- 每位學生的 Run 預設每分鐘 10 次、Submit 每分鐘 5 次。可以依組別或個人調整，系統上限分別是 30 和 15。
- 每位學生最多同時有 2 筆評測進行中。
- 程式碼與自訂輸入各最多 64 KiB；API 請求本文最多 16 MiB（匯入用）。
- 登入失敗限制：同一 IP 加同一帳號，5 分鐘內最多 10 次；同一 IP 最多 300 次（整班學生可能共用一個 NAT 出口，所以每 IP 的限制較寬鬆）。
- runner 整體記憶體 `RUNNER_MEM_LIMIT`（預設 1 GiB）、`pids_limit` 512；同時執行的沙箱數為 `RUNNER_CONCURRENCY`。

## 備份與還原

```sh
docker compose exec -T db pg_dump -U cyculcal -Fc cyculcal > backup-$(date +%F).dump
docker compose exec -T db pg_restore -U cyculcal -d cyculcal --clean --if-exists < backup.dump
```

所有狀態都在 PostgreSQL 裡（題目與所有 revision、提交、草稿、教學文案、設定、稽核紀錄），runner 是無狀態的。也可以另外用 `GET /api/v1/admin/export` 匯出題目 JSON。

## 升級

```sh
git pull
BUILDX_BUILDER=ccal-capped docker compose up -d --build --wait
```

資料庫遷移在 app 啟動時自動執行。如果要先單獨執行：`docker compose run --rm app /app/cyculcal migrate`。

## 帳號

- 第一個擁有者只能用 CLI 建立：`cyculcal create-owner`。已經有擁有者時會拒絕，除非加上 `--force`。系統沒有預設帳號或密碼。
- 擁有者重設學生密碼時，會產生一次性密碼，學生下次登入必須修改。
- 忘記擁有者密碼：用 `create-owner --force` 建立另一個擁有者，再由它重設。

## 環境變數

| 變數 | 說明 | 預設 |
|---|---|---|
| `DATABASE_URL` | PostgreSQL 連線字串 | （必填） |
| `RUNNER_URL` / `RUNNER_TOKEN` | runner 位址與共用密鑰（≥ 24 字元） | `http://runner:7070` / （必填） |
| `BIND` | API 監聽位址 | `0.0.0.0:8080` |
| `STATIC_DIR` | 前端靜態檔目錄 | （容器內為 `/app/web`） |
| `PUBLIC_ORIGINS` | 允許的瀏覽器 Origin（逗號分隔） | 空 |
| `COOKIE_SECURE` | cookie 加上 Secure | `false` |
| `SESSION_TTL_HOURS` | session 閒置期限 | 336（14 天） |
| `JUDGE_WORKERS` | 評測 worker 數 | 2 |
| `JUDGE_MAX_ATTEMPTS` | 每筆提交最多嘗試次數 | 3 |
| `RUNNER_TIMEOUT_S` | 呼叫 runner 的逾時 | 90 |
| `RUNNER_CONCURRENCY` | runner 同時執行的沙箱數 | 2 |
| `RUNNER_MAX_TIME_MS` / `RUNNER_MAX_WALL_TIME_MS` / `RUNNER_MAX_MEMORY_KB` / `RUNNER_MAX_OUTPUT_KB` | runner 的硬性上限 | 5000 / 16000 / 262144 / 1024 |
