# 匯入／匯出格式 `cycu-cal.import/v1`

這是一份 JSON 文件，`POST /api/v1/admin/import` 與 `GET /api/v1/admin/export` 使用同一個格式。未知的欄位會被拒絕，避免拼錯的欄位被悄悄忽略。

```jsonc
{
  "format_version": "cycu-cal.import/v1",   // 必填，目前唯一支援的版本
  "problems": [
    {
      "slug": "a-plus-b",                    // 穩定識別碼：小寫英數字與連字號，3–64 字元
      "content": {                           // ProblemContent，見下方
        "title": "A + B",
        "statement_md": "輸入兩個整數…",
        "examples": [{"input": "3 5\n", "output": "8\n", "explanation_md": "可省略"}],
        "mode": "full",                      // full | fill_in
        "starter_code": "#include <stdio.h>\n…",
        "template": null,                    // fill_in 才需要
        "limits": {"time_ms": 1000, "memory_kb": 65536, "output_kb": 64},
        "concepts": ["io", "variables"],
        "compare_mode": "trim_trailing_whitespace",   // 或 exact
        "reference_solution": "…",           // full 模式；fill_in 改用 reference_blanks
        "reference_blanks": null,
        "tests": [
          {"name": "範例", "visibility": "public", "input": "3 5\n", "expected_output": "8\n"},
          {"visibility": "hidden", "input": "1000 1000\n", "expected_output": "2000\n"}
        ]
      }
    }
  ],
  "problem_sets": [
    {
      "slug": "week-1-io",
      "title": "第 1 週：輸入與輸出",
      "description_md": "…",
      "learned_concepts": ["io", "variables"],
      "listed": true,                        // false = 只有被授權的組別或學生看得到
      "problems": ["hello-cycu", "a-plus-b"] // 依顯示順序；可引用文件內或題庫中已有的題目
    }
  ]
}
```

## 語意

| 情況 | 結果 |
|---|---|
| slug 不存在 | `create`：建立題目（revision 1，草稿） |
| slug 存在，內容雜湊相同 | `unchanged`：不寫入任何資料 |
| slug 存在，內容不同 | `update`：建立新的 revision；已發布的版本不受影響，要另外 publish |
| 題目集 slug 已存在 | 更新名稱、說明、概念與題目順序（只改關聯，不影響題庫） |
| 任何一項有錯 | 什麼都不寫入（同一個交易，全部成功或全部失敗） |

- 匯入**永遠不會發布**任何東西，也不會改變目前啟用的題目集。
- 已封存的題目不能透過匯入修改。
- 已發布的題目集若在匯入後會包含未發布的題目，整份匯入會被拒絕，維持「已發布的題目集只包含已發布的題目」這個規則。

## Dry-run 與錯誤回報

`POST /api/v1/admin/import?dry_run=true` 會回傳：

```json
{
  "dry_run": true,
  "ok": false,
  "errors": [
    {"pointer": "/problems/1/content/limits/time_ms", "code": "out_of_range", "message": "時間限制必須介於 100 與 5000 毫秒之間"},
    {"pointer": "/problem_sets/0/problems/1", "code": "unknown_problem", "message": "找不到題目 `missing-one`（不在這份文件，也不在題庫中）"}
  ],
  "problems": [],
  "problem_sets": []
}
```

驗證通過時，`problems[]` 與 `problem_sets[]` 會列出每一項的 `pointer`、`slug`、`action`（以及實際匯入後的 `id`）。有錯誤時 HTTP 狀態是 `422`。

## 重試

- 同一份文件重送是安全的：所有項目都會是 `unchanged`。
- 搭配 `Idempotency-Key` 時，會原樣重播第一次的回應。若同一個 key 用在內容不同的文件上，會回 `409`。

## 範例

- `content/demo/cycu-cal-demo.json`：示範教材（自建）
- `examples/load-demo-content.sh`：dry-run → 匯入 → 發布 → 啟用
