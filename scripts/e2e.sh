#!/usr/bin/env bash
# End-to-end acceptance test on a clean, isolated Docker Compose project.
#
# Starts PostgreSQL + judge-runner + app from scratch (fresh volumes), then
# drives everything through the public HTTP API exactly like the docs say:
# owner bootstrap → API token → agent import/publish/activate → student work
# with real GCC/isolate judging → set switching → revision traceability →
# hidden-test protection → fill-in bypass → judge outage & recovery →
# API restart during judging.
#
#   scripts/e2e.sh            # build, test, tear down
#   E2E_KEEP=1 scripts/e2e.sh # leave the stack running afterwards
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROJECT=cyculcal-e2e
PORT="${E2E_PORT:-18181}"
BASE="http://127.0.0.1:$PORT"
API="$BASE/api/v1"
WORK="$(mktemp -d)"
ENVF="$WORK/.env"
PASS=0
FAILED=0

cat > "$ENVF" <<EOF
POSTGRES_PASSWORD=$(openssl rand -hex 16)
RUNNER_TOKEN=$(openssl rand -hex 24)
APP_PORT=$PORT
RUNNER_CONCURRENCY=2
EOF
DC=(docker compose -p "$PROJECT" --env-file "$ENVF" -f docker-compose.yml)

cleanup() {
  if [[ "${E2E_KEEP:-0}" != 1 ]]; then "${DC[@]}" down -v >/dev/null 2>&1 || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

ok() { PASS=$((PASS + 1)); printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad() { FAILED=$((FAILED + 1)); printf '  \033[31m✗ %s\033[0m\n' "$1"; }
check() { # check "description" 'shell expression evaluated in this shell'
  local d=$1; shift
  local out
  if out=$(eval "$*" 2>&1); then ok "$d"; else bad "$d"; printf '    %s\n' "$(head -c 600 <<<"$out")"; fi
}
jqf() { jq -e "$2" "$1" >/dev/null 2>&1; } # jqf FILE EXPR
section() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# --- HTTP helpers ------------------------------------------------------------
# Session clients keep cookies in $WORK/<name>.jar and the CSRF token in $WORK/<name>.csrf.
login() { # login NAME USER PASSWORD
  curl -sS -c "$WORK/$1.jar" -H 'Content-Type: application/json' "$API/auth/login" \
    -d "$(jq -n --arg u "$2" --arg p "$3" '{username:$u,password:$p}')" | jq -r .csrf_token > "$WORK/$1.csrf"
}
as() { # as NAME METHOD PATH [JSON]  → prints body; HTTP status in $WORK/status
  local name=$1 method=$2 path=$3 body=${4:-}
  curl -sS -o "$WORK/body" -w '%{http_code}' -X "$method" "$API$path" -b "$WORK/$name.jar" \
    -H 'Content-Type: application/json' -H "X-CSRF-Token: $(cat "$WORK/$name.csrf")" ${body:+--data "$body"} > "$WORK/status"
  cat "$WORK/body"
}
tok() { # tok METHOD PATH [JSON] (uses API token)
  local method=$1 path=$2 body=${3:-}
  curl -sS -o "$WORK/body" -w '%{http_code}' -X "$method" "$API$path" -H "Authorization: Bearer $TOKEN" \
    -H 'Content-Type: application/json' ${body:+--data "$body"} > "$WORK/status"
  cat "$WORK/body"
}
status() { cat "$WORK/status"; }
wait_judged() { # wait_judged NAME SUBMISSION_ID [timeout_s]
  local deadline=$((SECONDS + ${3:-90})) st
  while ((SECONDS < deadline)); do
    st=$(as "$1" GET "/submissions/$2" | jq -r .status)
    [[ $st == done || $st == system_error ]] && { cat "$WORK/body"; return 0; }
    sleep 0.5
  done
  cat "$WORK/body"
  return 1
}
submit() { # submit NAME PROBLEM_ID KIND CODE → judged submission JSON in $WORK/r.json
  local id
  id=$(as "$1" POST /submissions "$(jq -n --arg p "$2" --arg k "$3" --arg c "$4" '{problem_id:$p, kind:$k, code:$c}')" | jq -r .id)
  wait_judged "$1" "$id" > "$WORK/r.json" || true
}
R="$WORK/r.json"

# --- 1. Clean start ----------------------------------------------------------
section "1. 從乾淨環境啟動（docker compose，全新 volume）"
"${DC[@]}" down -v >/dev/null 2>&1 || true
BUILDX_BUILDER="${BUILDX_BUILDER:-ccal-capped}" "${DC[@]}" up -d --build --wait 2>&1 | tail -3
check "app 健康檢查通過" 'curl -sf $BASE/healthz'
check "OpenAPI 文件可取得" 'curl -sf $API/openapi.json | jq -e ".paths | length > 50"'
check "Web 介面（SPA）可取得" 'curl -sf $BASE/ | grep -qi "id=\"root\""'
check "SPA 路由回傳 index.html" 'curl -sf $BASE/admin/problems | grep -qi "id=\"root\""'
check "沒有預設帳號可以登入" '[[ $(curl -s -o /dev/null -w "%{http_code}" -H "Content-Type: application/json" $API/auth/login -d "{\"username\":\"admin\",\"password\":\"admin\"}") == 401 ]]'

OWNER_PW=$(openssl rand -hex 12)
"${DC[@]}" exec -T -e OWNER_PW="$OWNER_PW" app /app/cyculcal create-owner --username owner --display-name 擁有者 --password-env OWNER_PW >/dev/null
check "已有擁有者時 create-owner 會被拒絕" '! "${DC[@]}" exec -T -e OWNER_PW="$OWNER_PW" app /app/cyculcal create-owner --username owner2 --password-env OWNER_PW'
login owner owner "$OWNER_PW"
check "擁有者登入成功" '[[ $(cat $WORK/owner.csrf) != null ]]'

TOKEN=$(as owner POST /admin/tokens '{"name":"e2e-agent","scopes":["problems:read","problems:write","sets:write","publish","submissions:read","content:write","students:read","students:write"]}' | jq -r .token)
check "建立 API token（ccal_ 開頭，只顯示一次）" '[[ $TOKEN == ccal_* ]] && [[ $(as owner GET /admin/tokens | jq "[.[]|has(\"token\")]|any") == false ]]'

# --- 2. Agent-only content workflow ------------------------------------------
section "2. 完全透過 API 建立、驗證、發布及啟用"
CCAL_URL=$BASE CCAL_TOKEN=$TOKEN examples/load-demo-content.sh > "$WORK/load.log" 2>&1 || true
check "示範教材匯入並全部發布（7 題 + 2 題目集）" '[[ $(grep -c "\"status\":\"published\"" $WORK/load.log) == 9 ]]'
CCAL_URL=$BASE CCAL_TOKEN=$TOKEN examples/agent-demo.sh > "$WORK/agent.log" 2>&1 || true
check "agent-demo.sh 完成（建題→驗證→發布→題目集→啟用）" 'grep -q "✓ 完成" $WORK/agent.log'
check "驗證使用真實評測（參考解答 accepted）" 'grep -q "\"reference_verdict\": \"accepted\"" $WORK/agent.log'
CCAL_URL=$BASE CCAL_TOKEN=$TOKEN examples/agent-demo.sh > "$WORK/agent2.log" 2>&1 || true
check "重跑 agent-demo 不產生新版本" 'grep -q "\"revision_created\": false" $WORK/agent2.log'
CCAL_URL=$BASE CCAL_TOKEN=$TOKEN examples/load-demo-content.sh > "$WORK/load2.log" 2>&1 || true
# (the real import re-sends the same Idempotency-Key and correctly replays the first response, so check the dry-run part)
check "重新匯入：dry-run 沒有任何 create，題庫仍為 8 題" '! sed -n "1,/^import:/p" $WORK/load2.log | grep -q ": create" && [[ $(tok GET "/admin/problems?limit=100" | jq ".items | length") == 8 ]]'

# --- 3. Student flow with the real judge -------------------------------------
section "3. 學生作答：通過、答案錯誤、編譯失敗、執行錯誤、逾時"
as owner POST /admin/users '{"username":"stu1","display_name":"學生一","password":"student-pass-1"}' >/dev/null
as owner POST /admin/users '{"username":"stu2","display_name":"學生二","password":"student-pass-2"}' >/dev/null
login stu1 stu1 student-pass-1
login stu2 stu2 student-pass-2
ACTIVE=$(as stu1 GET /site | jq -r .default_set_id)
# load-demo-content.sh (re-run above) activates week-1-io
check "學生預設看到目前啟用的題目集" '[[ $(as stu1 GET /problem-sets/$ACTIVE | jq -r .slug) == week-1-io ]]'
AB=$(as stu1 GET /problems/a-plus-b | jq -r .id)
AB_OK=$'#include <stdio.h>\nint main(void) {\n    int a, b;\n    scanf("%d %d", &a, &b);\n    printf("%d\\n", a + b);\n    return 0;\n}\n'
submit stu1 "$AB" submit "$AB_OK"
check "正確程式 → accepted（5/5）" 'jqf $R ".verdict==\"accepted\" and .passed==5"'
submit stu1 "$AB" run $'#include <stdio.h>\nint main(void){int a,b;scanf("%d %d",&a,&b);printf("%d\\n",a*b);return 0;}\n'
check "答案錯誤 → wrong_answer，附逐行差異" 'jqf $R ".verdict==\"wrong_answer\" and (.tests[0].diff|length>0)"'
submit stu1 "$AB" run $'#include <stdio.h>\nint main(void) {\n    int a, b\n    scanf("%d %d", &a, &b);\n    printf("%d\\n", a + b);\n    return 0;\n}\n'
cp "$R" "$WORK/ce.json"
check "編譯失敗 → compile_error，首要卡片是缺分號" 'jqf $R ".verdict==\"compile_error\" and .cards[0].rule==\"syntax.missing_semicolon\" and .cards[0].primary"'
check "卡片包含位置、原始訊息與逐層提示" 'jqf $R ".cards[0].span.line==4 and (.cards[0].raw|length>0) and .cards[0].hints_total>=2 and (.cards[0].where_text|test(\"第 3 行\"))"'
check "連鎖錯誤被標示（後面的未宣告 b）" 'jqf $R "[.cards[]|select(.maybe_cascade_of!=null)]|length>=1"'
CE_ID=$(jq -r .id "$R"); FID=$(jq -r '.cards[0].finding_id' "$R")
check "可展開下一層提示" '[[ $(as stu1 POST /submissions/$CE_ID/findings/$FID/hints "{}" | jq ".hints|length") == 1 ]]'
submit stu1 "$AB" run $'int main(void){ int *p = 0; return *p; }\n'
check "執行錯誤 → runtime_error，原因標示為推測" 'jqf $R ".verdict==\"runtime_error\" and ([.cards[]|select(.rule==\"run.segfault\" and .confidence==\"possible\")]|length==1)"'
submit stu1 "$AB" run $'#include <stdio.h>\nint main(void){int x,s=0; while(scanf("%d",&x)) s+=x; printf("%d\\n",s); return 0;}\n'
check "逾時 → time_limit，並指出 while(scanf) 的可能原因" 'jqf $R ".verdict==\"time_limit\" and ([.cards[]|select(.rule==\"run.time_limit\" and .confidence==\"possible\")]|length==1) and ([.cards[]|select(.rule==\"lint.while_scanf_no_compare\")]|length==1)"'
submit stu1 "$AB" run $'#include <stdio.h>\nint main(void){ int x = 1; if (x) { break; } return 0; }\n'
check "沒有專屬說明的錯誤 → 誠實的 fallback 卡片" 'jqf $R ".verdict==\"compile_error\" and .cards[0].rule==\"fallback.compile_error\""'
submit stu1 "$AB" run $'#include <stdio.h>\nint main(void){ Printf("hi\\n"); return 0; }\n'
check "拼錯函式（連結錯誤）→ 指出可能想寫 printf" 'jqf $R ".cards[0].rule==\"func.misspelled\" and (.cards[0].cause|test(\"printf\"))"'

# --- 4. Hidden tests, other students' data, admin functions ------------------
section "4. 隱藏測資、管理功能與他人提交的保護"
submit stu1 "$AB" submit $'#include <stdio.h>\nint main(void){int a,b;scanf("%d %d",&a,&b);printf("%d %d\\n",a,b);return 0;}\n'
SUB=$(jq -r .id "$R")
check "學生看不到隱藏測資的輸入、輸出與差異" '! grep -qE "2000|1999" $R && jqf $R "[.tests[]|select(.hidden)|.input,.expected_output,.actual_output,.diff]|all(.==null)"'
check "題目頁面不含隱藏測資與參考解答" '! as stu1 GET /problems/a-plus-b | grep -qE "2000|1999|reference"'
check "學生無法讀取他人提交（404）" 'as stu2 GET /submissions/$SUB; [[ $(status) == 404 ]]'
check "學生無法使用管理 API（403）" 'as stu1 GET /admin/problems/a-plus-b; [[ $(status) == 403 ]]'
check "缺少 CSRF token 的寫入被拒（403）" '[[ $(curl -s -o /dev/null -w "%{http_code}" -b $WORK/stu1.jar -H "Content-Type: application/json" -X PUT $API/me/drafts/$AB -d "{\"code\":\"x\"}") == 403 ]]'
check "擁有者可以看到完整內容" 'tok GET /admin/submissions/$SUB | grep -q 2000'

# --- 5. Set switching keeps drafts and in-flight work --------------------------
section "5. 切換題目集不需重啟、不遺失草稿、不改變已提交工作的依據"
as stu1 PUT "/me/drafts/$AB" '{"code":"// 還沒寫完的程式\n"}' >/dev/null
ID=$(as stu1 POST /submissions "$(jq -n --arg p "$AB" --arg c "$AB_OK" '{problem_id:$p, kind:"submit", code:$c}')" | jq -r .id)
tok PUT /admin/site/active-problem-set '{"problem_set":"week-1-io"}' >/dev/null
W1=$(tok GET /admin/problem-sets/week-1-io | jq -r .id)
check "切換後學生的預設題目集立即改變" '[[ $(as stu1 GET /site | jq -r .default_set_id) == $W1 ]]'
check "草稿保留" '[[ $(as stu1 GET /me/drafts/$AB | jq -r .code) == "// 還沒寫完的程式" ]]'
wait_judged stu1 "$ID" > "$R" || true
check "切換前送出的提交照常完成（revision 1）" 'jqf $R ".verdict==\"accepted\" and .revision_number==1"'

# --- 6. Shared problems ------------------------------------------------------
section "6. 同一題用在多個題目集；移除一個關聯不影響其他用途"
check "a-plus-b 同時在兩個題目集" '[[ $(tok GET /admin/problems/a-plus-b | jq ".sets|length") == 2 ]]'
ITEMS=$(tok GET /admin/problem-sets/week-2-loops-functions | jq -c '[.items[].slug|select(.!="a-plus-b")]')
tok PUT /admin/problem-sets/week-2-loops-functions/items "{\"problems\":$ITEMS}" >/dev/null
check "從第 2 週移除後，第 1 週仍包含它" 'tok GET /admin/problem-sets/week-1-io | jq -e "[.items[].slug]|index(\"a-plus-b\")!=null"'
check "題庫中的題目不受影響" '[[ $(tok GET /admin/problems/a-plus-b | jq -r .status) == published ]]'

# --- 7. Revision traceability ------------------------------------------------
section "7. 修改已發布題目後，舊提交仍可追溯原始依據"
tok GET /admin/problems/a-plus-b | jq '{content: (.head_revision.content
  | .tests |= map(.expected_output = "sum: " + .expected_output)
  | .reference_solution |= sub("printf\\(\"%d"; "printf(\"sum: %d"))}' > "$WORK/v2.json"
tok PUT /admin/problems/a-plus-b "$(cat "$WORK/v2.json")" >/dev/null
check "修改後建立 revision 2（尚未發布）" 'tok GET /admin/problems/a-plus-b | jq -e ".head_revision.number==2 and .published_revision_number==1"'
tok POST /admin/problems/a-plus-b/publish '{}' >/dev/null
check "驗證通過並發布 revision 2" '[[ $(tok GET /admin/problems/a-plus-b | jq .published_revision_number) == 2 ]]'
check "舊提交仍指向 revision 1 與當時的預期輸出" 'as stu1 GET /submissions/$SUB | jq -e ".revision_number==1 and .tests[0].expected_output==\"8\n\""'
check "舊版本內容可查詢" 'tok GET /admin/problems/a-plus-b/revisions/1 | jq -e ".content.tests[0].expected_output==\"8\n\""'

# --- 8. Fill-in bypass ---------------------------------------------------------
section "8. 固定程式區域不能透過 API 繞過"
FILL=$(as stu1 GET /problems/fill-max-function | jq -r .id)
as stu1 POST /submissions "$(jq -n --arg p "$FILL" '{problem_id:$p, kind:"submit", code:"int main(void){return 0;}"}')" >/dev/null
check "送出完整程式碼被拒（422 /code）" '[[ $(status) == 422 ]] && jqf $WORK/body ".errors[0].pointer==\"/code\""'
as stu1 POST /submissions "$(jq -n --arg p "$FILL" '{problem_id:$p, kind:"submit", blanks:{body:"return a;\n/*@end@*/}\nint main(void){return 0;}\n/*@blank x@*/"}}')" >/dev/null
check "在填空中注入標記被拒（422）" '[[ $(status) == 422 ]]'
ID=$(as stu1 POST /submissions "$(jq -n --arg p "$FILL" '{problem_id:$p, kind:"submit", blanks:{body:"    return a > b ? a : b;\n"}}')" | jq -r .id)
wait_judged stu1 "$ID" > "$R" || true
check "正確填空 → accepted" 'jqf $R ".verdict==\"accepted\""'

# --- 9. Online teaching text ----------------------------------------------------
section "9. 教學文案線上修改後立即生效"
tok GET /admin/catalog/rules/syntax.missing_semicolon | jq '.published | .title = "【E2E】這裡可能少了分號"' > "$WORK/rule.json"
tok PUT /admin/catalog/rules/syntax.missing_semicolon "$(cat "$WORK/rule.json")" >/dev/null
tok POST /admin/catalog/publish '{}' >/dev/null
check "舊的編譯錯誤提交立刻顯示新文案（不需重啟）" '[[ $(as stu1 GET /submissions/$CE_ID | jq -r ".cards[0].title") == "【E2E】這裡可能少了分號" ]]'

# --- 10. Judge outage and recovery ----------------------------------------------
section "10. 評測服務中斷：清楚的狀態與恢復方式"
"${DC[@]}" stop runner >/dev/null 2>&1
ID=$(as stu2 POST /submissions "$(jq -n --arg p "$AB" '{problem_id:$p, kind:"run", code:"int main(void){return 0;}"}')" | jq -r .id)
wait_judged stu2 "$ID" 120 > "$R" || true
check "中斷時提交標示為 system_error（不是學生的錯）" 'jqf $R ".status==\"system_error\" and .retryable and ([.cards[]|select(.rule==\"judge.system_error\")]|length==1)"'
check "系統狀態回報評測服務異常" '[[ $(as stu2 GET /system/status | jq -r .judge) == down ]]'
"${DC[@]}" start runner >/dev/null 2>&1
for _ in $(seq 1 30); do [[ $(docker inspect -f '{{.State.Health.Status}}' "$PROJECT-runner-1" 2>/dev/null) == healthy ]] && break; sleep 1; done
as stu2 POST "/submissions/$ID/rejudge" '{}' >/dev/null
wait_judged stu2 "$ID" > "$R" || true
check "恢復後重新評測成功" 'jqf $R ".status==\"done\""'

# --- 11. API restart while judging ------------------------------------------------
section "11. 評測途中重啟 app：工作不遺失、不重複計分"
SLEEPY=$'#include <stdio.h>\n#include <unistd.h>\nint main(void){ sleep(2); int a,b; scanf("%d %d",&a,&b); printf("sum: %d\\n",a+b); return 0; }\n'
ID=$(as stu2 POST /submissions "$(jq -n --arg p "$AB" --arg c "$SLEEPY" '{problem_id:$p, kind:"submit", code:$c}')" | jq -r .id)
sleep 1.5
"${DC[@]}" restart app >/dev/null 2>&1
for _ in $(seq 1 30); do curl -sf "$BASE/healthz" >/dev/null && break; sleep 1; done
wait_judged stu2 "$ID" 120 > "$R" || true
check "重啟後工作被取回並完成" 'jqf $R ".status==\"done\""'
check "取回後重新評測（attempts ≥ 2），只有一筆結果" 'tok GET /admin/submissions/$ID | jq -e ".attempts>=2" && [[ $(tok GET "/admin/submissions?kind=submit&problem_id=$AB" | jq "[.items[]|select(.id==\"$ID\")]|length") == 1 ]]'

printf '\n\033[1m結果：%d 通過，%d 失敗\033[0m\n' "$PASS" "$FAILED"
[[ $FAILED == 0 ]]
