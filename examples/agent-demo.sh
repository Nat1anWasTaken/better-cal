#!/usr/bin/env bash
# Agent demo: create a problem with tests, validate it with the real judge,
# publish it, put it in a new problem set, publish the set and make it the
# active set — using only the HTTP API and an API token (no browser).
#
#   export CCAL_URL=http://localhost:8080
#   export CCAL_TOKEN=ccal_...   # owner dashboard → API tokens, scopes:
#                                # problems:read problems:write sets:write publish
#   examples/agent-demo.sh
#
# Safe to re-run: identical content is a no-op and Idempotency-Keys make
# retried creates return the original response.
set -euo pipefail
: "${CCAL_URL:?set CCAL_URL, e.g. http://localhost:8080}"
: "${CCAL_TOKEN:?set CCAL_TOKEN to an API token}"
API="$CCAL_URL/api/v1"
SLUG="${DEMO_SLUG:-agent-even-odd}"
SET_SLUG="${DEMO_SET_SLUG:-agent-demo-set}"

call() { # call METHOD PATH [JSON] [extra curl args...]
  local method=$1 path=$2 body=${3:-}
  shift 3 || shift $#
  local out status
  out=$(mktemp)
  status=$(curl -sS -o "$out" -w '%{http_code}' -X "$method" "$API$path" \
    -H "Authorization: Bearer $CCAL_TOKEN" -H 'Content-Type: application/json' \
    ${body:+--data "$body"} "$@")
  if [[ $status -ge 400 ]]; then
    echo "✗ $method $path → HTTP $status" >&2
    jq . "$out" >&2 || cat "$out" >&2
    rm -f "$out"
    return 1
  fi
  cat "$out"
  rm -f "$out"
}

echo "1) 建立題目與測資（草稿）"
PROBLEM=$(jq -n --arg slug "$SLUG" '{
  slug: $slug,
  content: {
    title: "奇數還是偶數",
    statement_md: "> CYCU-CAL 自建教材（由 Agent 透過 API 建立）\n\n輸入一個整數 n，若 n 是偶數輸出 `even`，否則輸出 `odd`。",
    examples: [{input: "4\n", output: "even\n"}],
    mode: "full",
    starter_code: "#include <stdio.h>\n\nint main(void) {\n    int n;\n    scanf(\"%d\", &n);\n\n    return 0;\n}\n",
    concepts: ["io", "variables", "conditionals"],
    reference_solution: "#include <stdio.h>\n\nint main(void) {\n    int n;\n    scanf(\"%d\", &n);\n    if (n % 2 == 0) {\n        printf(\"even\\n\");\n    } else {\n        printf(\"odd\\n\");\n    }\n    return 0;\n}\n",
    tests: [
      {name: "範例", visibility: "public", input: "4\n", expected_output: "even\n"},
      {name: "奇數", visibility: "public", input: "7\n", expected_output: "odd\n"},
      {visibility: "hidden", input: "0\n", expected_output: "even\n"},
      {visibility: "hidden", input: "-3\n", expected_output: "odd\n"}
    ]
  }
}')
if call GET "/admin/problems/$SLUG" "" >/dev/null 2>&1; then
  # Already exists: PUT creates a new revision only if the content changed.
  call PUT "/admin/problems/$SLUG" "$(jq '{content}' <<<"$PROBLEM")" | jq '{slug, status, revision: .head_revision.number, revision_created}'
else
  call POST /admin/problems "$PROBLEM" -H "Idempotency-Key: create-$SLUG" | jq '{slug, status, revision: .head_revision.number}'
fi

echo "2) 驗證（用參考解答跑真實評測）"
call POST "/admin/problems/$SLUG/validate" "{}" | jq '{ok, revision_number, errors, reference_verdict: .reference_run.verdict}'

echo "3) 發布題目"
call POST "/admin/problems/$SLUG/publish" "{}" | jq '{slug, status, published_revision_number}'

echo "4) 建立題目集（草稿）並加入題目"
SET_BODY=$(jq -n --arg slug "$SET_SLUG" --arg p "$SLUG" '{slug: $slug, title: "Agent 示範題目集", description_md: "CYCU-CAL 自建教材，由 API 建立。", learned_concepts: ["io", "variables", "conditionals"], problems: [$p]}')
if call GET "/admin/problem-sets/$SET_SLUG" "" >/dev/null 2>&1; then
  call PUT "/admin/problem-sets/$SET_SLUG" "$SET_BODY" | jq '{slug, status, items: [.items[].slug]}'
else
  call POST /admin/problem-sets "$SET_BODY" -H "Idempotency-Key: create-set-$SET_SLUG" | jq '{slug, status, items: [.items[].slug]}'
fi

echo "5) 發布題目集"
call POST "/admin/problem-sets/$SET_SLUG/publish" "{}" | jq '{slug, status}'

echo "6) 設為目前啟用的題目集"
call PUT /admin/site/active-problem-set "$(jq -n --arg s "$SET_SLUG" '{problem_set: $s}')" | jq .

echo "✓ 完成：學生進入平台時預設會看到「Agent 示範題目集」。"
