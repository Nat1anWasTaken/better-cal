#!/usr/bin/env bash
# Import the demo teaching material (content/demo), validate + publish every
# problem, publish both sets and activate week 1. Uses only the API.
#   CCAL_URL=http://localhost:8080 CCAL_TOKEN=ccal_... examples/load-demo-content.sh
set -euo pipefail
: "${CCAL_URL:?}"; : "${CCAL_TOKEN:?}"
API="$CCAL_URL/api/v1"
DOC="$(dirname "$0")/../content/demo/cycu-cal-demo.json"
H=(-H "Authorization: Bearer $CCAL_TOKEN" -H 'Content-Type: application/json')

echo "dry-run:"
curl -sS -X POST "$API/admin/import?dry_run=true" "${H[@]}" --data @"$DOC" | jq '{ok, errors, problems: [.problems[] | "\(.slug): \(.action)"], problem_sets: [.problem_sets[] | "\(.slug): \(.action)"]}'
echo "import:"
curl -sS -X POST "$API/admin/import" "${H[@]}" -H "Idempotency-Key: demo-$(sha256sum "$DOC" | cut -c1-16)" --data @"$DOC" | jq '{ok, problems: [.problems[] | "\(.slug): \(.action)"]}'
for slug in $(jq -r '.problems[].slug' "$DOC"); do
  printf 'publish %-22s ' "$slug"
  curl -sS -X POST "$API/admin/problems/$slug/publish" "${H[@]}" --data '{}' | jq -c '{status, published_revision_number, errors}'
done
for set in $(jq -r '.problem_sets[].slug' "$DOC"); do
  printf 'publish set %-22s ' "$set"
  curl -sS -X POST "$API/admin/problem-sets/$set/publish" "${H[@]}" | jq -c '{status}'
done
curl -sS -X PUT "$API/admin/site/active-problem-set" "${H[@]}" --data '{"problem_set":"week-1-io"}' | jq -c .
