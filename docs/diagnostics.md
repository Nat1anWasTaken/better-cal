# 新手錯誤提示（診斷轉譯層）

目標不是把 compiler error 翻成中文，而是用新手聽得懂的話說明：

- 發生了什麼事
- 從哪裡看起
- 為什麼有問題
- 下一步可以做什麼

同時誠實區分「編譯器確認的事實」與「系統推測的原因」。

## 架構

```
GCC JSON 診斷 ─┐
ld 文字輸出 ───┼─▶ parse ─▶ normalize ─▶ 規則（含程式碼證據）─▶ 排序／連鎖標記 ─▶ Finding[]
沙箱執行結果 ──┤                                                                 │
學生原始碼 ────┘                                                                 ▼
                         教學文案（DB，可線上編輯）＋ 學生已學概念 ─▶ render ─▶ 卡片
```

程式碼位於 `backend/crates/diagnostics`，是純函式庫（不做 I/O），可以獨立測試。

| 模組 | 職責 |
|---|---|
| `gcc.rs` | 解析 `-fdiagnostics-format=json`（GCC 12 schema） |
| `linker.rs` | 解析 ld 文字：`undefined reference`、`multiple definition` |
| `source.rs` | 容錯的小型 C lexer。回答規則需要的證據問題，例如：scanf 的第 2 個參數文字是什麼、哪個 `{` 沒有關、這個名稱是否在別的區塊宣告過、是否有全形標點 |
| `rules/compile.rs` | 編譯與連結規則（各有 matcher 與證據檢查） |
| `rules/lint.rs` | GCC 不一定會報的新手錯誤，例如 `for (...);`、`while (scanf(...))`、`scanf("%d\n")` |
| `rules/runtime.rs` | 執行結果：segfault、SIGFPE、非 0 回傳、逾時、記憶體超量、輸出超量、答案錯誤、空白差異、服務故障 |
| `rank.rs` | 決定「建議先處理」的那一則，並標記可能的連鎖錯誤 |
| `catalog.rs`、`template.rs`、`render.rs` | 教學文案的模型與驗證、模板引擎、依已學概念選擇措辭 |

### Finding 與卡片

`analyze()` 只產生結構化的 `Finding`，內容包括：

- 規則 id
- 分類：編譯錯誤、連結錯誤、警告、執行問題、答案錯誤、服務故障
- 嚴重度
- 信心等級：
  - `confirmed`：編譯器或程式碼直接看得到
  - `likely`：診斷加上程式碼證據
  - `possible`：推測
- 位置與精確度：
  - `exact`：標示的位置就是問題所在
  - `near`：編譯器在這裡發現，原因可能在前面
  - `approximate`：沒有行號，位置是推測的
- 證據變數，例如 `ident`、`prev_line`、`suggestion`
- 原始診斷（所有相關的原文，給進階學生與教學者）
- 是否位於固定程式區域
- 是否可能是連鎖反應

`render()` 在**顯示時**才套用教學文案，所以文案修改後，舊的提交也會看到新說明，而評測依據完全不受影響。

### 卡片欄位與畫面上的區塊

| 欄位 | 卡片上的區塊 |
|---|---|
| `title` | 標題 |
| `what` | 發生什麼事（事實） |
| `cause` | 可能原因，與 `confidence_label` 一起顯示 |
| `where_text` | 從哪裡看起，加上 `precision_note` |
| `why` | 為什麼有問題（只補當下需要的概念） |
| `next_steps` | 下一步 |
| `hints[]` | 逐層提示：先給檢查方向，最後才是一般化的範例，不是學生程式的修正版 |
| `raw` | 原始訊息 |

API 依學生的設定決定一開始顯示幾層提示（`auto_reveal_hints`）以及最多能展開幾層（`max_hint_levels`）。每次展開都會被記錄，供教學統計使用。

## 正確性原則

- **不假設 Clang，也不假設有結構化診斷**：實際部署的是 GCC 12.2，見 [toolchain.md](toolchain.md)。結構化資訊來自 GCC JSON；只有連結器需要解析文字，而且解析器是獨立模組並有固定測試。
- **警告不是失敗**：編譯成功但有警告時，評測照常進行，警告以「建議檢查」卡片呈現。
- **不武斷認定連鎖錯誤**：第一個語法錯誤之後、同一個函式內的其他錯誤，只標記為「可能是連鎖反應」並收合起來，不會隱藏，也不斷言因果。
- **要有證據才下結論**：
  - 「scanf 少了 `&`」只在 GCC 指出參數型別是 `int`（而格式要求 `int *`），**而且**程式碼裡那個參數是單純的變數名稱時，才標為 `likely`。
  - `char c; scanf("%d", &c)` 是格式選錯，不是少 `&`。
  - `char name[20]; scanf("%s", &name)` 是另一張卡，說明陣列不需要 `&`。
- **不轉述會誤導人的建議**：GCC 對 `string name;` 會建議 `did you mean 'stdin'?`，這個建議永遠不會出現在卡片上。
- **逾時不等於無窮迴圈，崩潰不等於陣列越界**：
  - 這些卡片的原因一律標為推測。
  - 會列出幾個可能性，並指向相關的警告（例如第 5 行的 scanf 警告），或指向 `while (scanf(...))`。
  - 在沙箱裡 stdin 是檔案，讀到結尾時 scanf 會回傳 EOF（-1）。所以「卡在等待輸入」不會發生；真正常見的原因是迴圈在 EOF 之後停不下來。
- **未知的診斷**：使用誠實的備援卡片，說明「系統還沒有這種錯誤的專屬說明」，仍然提供位置、原文與通用的檢查方法。絕不套用不相關的模板。
- **隱藏測資**：分析隱藏測資時只使用它的狀態（例如 segfault），不讀取它的 stdout 或 stderr，所以卡片上不可能出現隱藏測資的內容。
- **服務故障**：使用獨立的 `judge.system_error` 卡片，明確說明「不是你的程式有錯」。

## 涵蓋的規則

完整列表見 `rules/mod.rs` 的 `SPECS`，也可以在管理介面的「教學文案」查看。

| 類別 | 規則 |
|---|---|
| 分號與括號 | `syntax.missing_semicolon`、`missing_close_paren`、`missing_open_paren`、`unclosed_brace`（依縮排推測是哪個 `{`）、`extra_brace`、`unterminated_string`、`fullwidth_char`（全形 `；（）` 等，直接從原始碼確認）、`stray_char`、`expected_expression`、`unexpected_token` |
| 未宣告識別字 | `ident.undeclared`（區分拼字錯誤、宣告在另一個區塊、尚未宣告）、`ident.unknown_type`（`string`、`bool`）、`ident.redefinition` |
| 函式宣告與呼叫 | `func.missing_include`（依 GCC note 指出標頭檔）、`func.misspelled`（隱含宣告與連結錯誤合併成一張）、`func.used_before_definition`（合併後續的 conflicting types 與格式警告）、`func.undefined`、`too_few_args`、`too_many_args`、`conflicting_types`、`missing_return`、`void_return_value`、`return_without_value` |
| 型別、格式與參數 | `format.scanf_missing_address`、`scanf_string_address`、`scanf_type_mismatch`（`%f` 對 `double *` 建議改用 `%lf`）、`printf_type_mismatch`、`missing_argument`、`extra_argument` |
| 連結錯誤 | `link.undefined_main`（找出 `mian` 這類相近名稱）、`link.undefined_function`（找出 `ad` / `add` 這類相近名稱）、`link.multiple_definition` |
| 警告（lint） | 未使用變數或參數、未初始化、`if (x = 0)`、空的 if 主體、縮排誤導、signed 與 unsigned 比較、`0 < x < 10`、沒有效果的敘述（`x == 5;`）、迴圈多跑一次 |
| 自訂 lint | `lint.loop_empty_body`、`lint.while_scanf_no_compare`、`lint.scanf_trailing_whitespace` |
| 執行與評測 | `run.segfault`、`run.fpe`、`run.abort`、`run.signal`、`run.nonzero_exit`、`run.time_limit`、`run.memory_limit`、`run.output_limit`、`judge.wrong_answer`、`judge.whitespace_only`、`judge.system_error`、`compile.timeout` |
| 備援 | `fallback.compile_error`、`fallback.warning`、`fallback.link_error` |

## 測試

### 真實 GCC 輸出的 fixtures

`backend/crates/diagnostics/fixtures/<case>/` 下每個案例包含：

| 檔案 | 內容 |
|---|---|
| `main.c` | 學生程式 |
| `stderr.txt`、`exit_code.txt` | 用 runner 映像內的真實 GCC 12.2、相同選項產生 |
| `expect.toml` | 預期的首要規則、行號、證據變數、必須出現的規則、**不可出現的規則（反例）**、應被標為連鎖的規則 |

目前有 54 個 fixtures，其中 7 個是「正確程式必須沒有任何卡片」的反例。其他反例散布在各案例中，例如：

- 全形括號產生的 `expected ... ';' ...` 訊息不可被當成少分號。
- `&c` 對到 `%d` 不可說成少 `&`。

重新產生 fixtures：`scripts/gen-fixtures.sh`。`fixtures/TOOLCHAIN` 記錄當時的編譯器版本，有測試檢查它。

### 其他測試

- `tests/catalog_render.rs`：
  - 內建文案能完整渲染。
  - 每條規則都有文案。
  - 模板變數驗證會指出錯誤的欄位。
  - 措辭會依已學概念改變（scanf 在學過指標前後的說法不同）。
  - 線上文案損壞時自動退回內建版本。
  - 停用規則時改用 fallback。
  - 自訂規則一律標為推測。
  - 隱藏測資的 stderr 不會出現在任何地方。
- 單元測試：lexer、模板、linker parser、GCC JSON parser。

## 線上編輯教學文案

擁有者在「管理 → 教學文案」可以：

- 修改每條規則的標題、各段文字與逐層提示。
- 依已學概念（例如 `pointers`）提供不同措辭的版本。
- 停用規則，或調整優先度（影響「建議先處理」的排序）。
- 修改共用文字：信心標籤、連鎖說明、比對規則說明、隱藏測資提示。
- 建立自訂規則。只能比對 GCC 診斷的種類、警告選項與訊息的正規表達式（Rust `regex` 引擎，線性時間，沒有 ReDoS 風險）。存檔前必須提供至少一個正例程式，並經真實 GCC 編譯確認比對得到；可以加上反例。自訂規則一律標為推測。

所有修改先存成**草稿**，可以用貼上的程式碼、範例或真實提交預覽，並勾選不同的已學概念。按「發布全部」後立即生效，不需重啟。發布歷史可以查詢；也可以「還原成內建版本」。

### 模板語法

| 語法 | 意思 |
|---|---|
| `{var}` | 插入證據變數 |
| `{?var}…{/var}` | 變數存在時才顯示 |
| `{!var}…{/var}` | 變數不存在時才顯示 |
| `{{`、`}}` | 輸出大括號（一般的 C 程式碼不需要跳脫） |

存檔時會驗證，每條規則只能使用它宣告的變數，管理介面會列出可用的變數。

內建文案（`backend/crates/diagnostics/catalog/*.toml`）是初始內容與「還原」的基準。資料庫中線上發布的版本優先。

## 新增一條內建規則

1. 在 `fixtures/` 加入能觸發它的程式（正例）與容易誤判的程式（反例），執行 `scripts/gen-fixtures.sh`。
2. 在 `rules/mod.rs` 的 `SPECS` 登記規則 id、證據變數與預設權重。
3. 在 `rules/compile.rs`（或 `lint.rs`、`runtime.rs`）寫 matcher 與證據檢查。
4. 在 `catalog/*.toml` 寫文案（測試會強制每條規則都有文案）。
5. 撰寫 `expect.toml`，執行 `cargo test -p diagnostics`。
