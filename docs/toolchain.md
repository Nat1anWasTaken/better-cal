# 評測工具鏈（固定版本）

學生程式一律在 **judge-runner** 容器中，以下列固定的工具鏈編譯與執行。請求端（包含 API 與擁有者）**無法**覆寫編譯器、C 標準或編譯選項。

| 項目 | 值 |
|---|---|
| 編譯器 | GCC **12.2.0**（Debian 12 套件 `gcc-12=12.2.0-14+deb12u1`） |
| C 標準 | **C17**（`-std=c17`） |
| 連結器 | GNU ld 2.40（Debian 12 binutils） |
| C 函式庫 | glibc 2.36（Debian 12） |
| 沙箱 | [ioi/isolate](https://github.com/ioi/isolate) **v2.7**（cgroup v2） |
| 基底映像 | `debian:bookworm-slim` |

## 編譯選項

定義於 `backend/crates/runner/src/sandbox.rs` 的 `COMPILE_FLAGS`：

```
gcc-12 -std=c17 -O2 -pipe -Wall -Wextra -Wformat=2 \
       -fdiagnostics-format=json -fno-diagnostics-color \
       -o main main.c -lm
```

- `-Wall -Wextra -Wformat=2`：讓新手常見問題（scanf 少 `&`、未初始化、`if (x = 0)`、空的 if 主體…）以警告呈現。
- `-O2`：讓 `-Wmaybe-uninitialized`、`-Waggressive-loop-optimizations` 等需要資料流分析的警告能出現。注意：最佳化可能會移除沒有被使用的計算，這是 C 語言的正常行為。
- `-fdiagnostics-format=json`：GCC 12 的結構化診斷（位置、範圍、選項、子訊息、fix-it）。診斷轉譯層以它為主要資料來源，不解析英文文字。
- 連結器（ld）沒有 JSON 模式，所以它的文字輸出（`undefined reference to`、`multiple definition of`）交給獨立的 parser（`backend/crates/diagnostics/src/linker.rs`）處理，並有固定的測試。
- `-lm` 永遠連結，學生使用 `<math.h>` 不需額外設定。
- 編譯時使用 `LANG=C`，確保訊息為英文，與測試 fixtures 一致。

在 GCC 12 + C17 下，「隱含宣告函式」（implicit declaration）是**警告**而不是錯誤。所以拼錯 `printf` 時，錯誤是在連結階段才出現；診斷層會把這兩則訊息合併成一張說明卡。

## 執行限制

| 限制 | 預設值（每題可設定） | runner 上限 |
|---|---|---|
| CPU 時間 | 1000 ms | 5000 ms |
| 牆上時間（wall time） | CPU 時間 × 3 + 1 s | 16 s |
| 記憶體（cgroup） | 64 MiB | 256 MiB |
| 輸出量 | 64 KiB | 1024 KiB |
| 行程數 | 1（不能 fork） | — |
| 網路 | 無（isolate 為每個沙箱建立空的網路命名空間） | — |
| 編譯 | 10 s CPU、20 s 牆上時間、512 MiB、64 行程 | — |

runner 會再以自己的上限截斷請求的值（`RUNNER_MAX_*` 環境變數）。swap 一律關閉（`memory.swap.max = 0`），避免記憶體限制被繞過。

## 確認實際版本

```sh
docker compose exec runner gcc-12 --version
docker compose exec runner isolate --version
# 或透過 API（需要 runner token，只能在內部網路使用）：GET http://runner:7070/v1/info
```

擁有者的「總覽」頁面也會顯示 runner 回報的編譯器版本與編譯選項。

## 更新工具鏈

1. 修改 `deploy/Dockerfile`（target `runner`） 中固定的套件版本（或 isolate tag）。
2. 重新建置 runner 映像：`BUILDX_BUILDER=ccal-capped docker compose build runner`。
3. 用新的編譯器重新產生診斷 fixtures：`scripts/gen-fixtures.sh`。
4. 執行 `cargo test -p diagnostics`，確認每一條規則仍然對得上新的訊息，再更新本文件。

## 為什麼不是 Judge0？

需求原本指定 Judge0，但這台主機（以及大多數新的 Linux 發行版，包括 Ubuntu 22.04+ 與 24.04）使用 **cgroup v2**。Judge0 最新版（1.13.1）內建的 isolate 只支援 cgroup v1，官方文件要求修改 GRUB 切回 v1 並重開機，這會影響主機上的其他服務（例如 Dokploy）。

經使用者同意，改用同樣以 isolate 為核心、但原生支援 cgroup v2 的自建 runner：

- 安全模型相同：特權容器加上 isolate 沙箱，只放在內部網路。
- 可以固定編譯器版本與選項，並取得 GCC 的 JSON 診斷。
- 後端透過 `RunnerClient` 這個窄介面存取 runner，日後要換成 Judge0 只需要另寫一個 adapter。
