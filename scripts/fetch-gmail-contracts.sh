#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DISCOVERY_URL="${OPENCRAW_GMAIL_DISCOVERY_URL:-https://gmail.googleapis.com/\$discovery/rest?version=v1}"
OUT_DIR="${1:-contracts/gmail}"
NOW_UTC="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

require_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $name" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd jq
require_cmd shasum
require_cmd date

mkdir -p "$OUT_DIR"

DISCOVERY_FILE="$OUT_DIR/discovery-rest-v1.json"
echo "Fetching Gmail discovery contract from $DISCOVERY_URL"
curl -fsS "$DISCOVERY_URL" > "$DISCOVERY_FILE"

SCHEMA_SHA="$(shasum -a 256 "$DISCOVERY_FILE" | awk '{print $1}')"

jq '
def walk_methods($prefix):
  [
    (.methods // {} | to_entries[]?
      | {
          operation: ($prefix + .key),
          httpMethod: .value.httpMethod,
          path: .value.path,
          id: .value.id,
          description: .value.description,
          scopes: (.value.scopes // []),
          parameters: ((.value.parameters // {}) | keys)
        }
    ),
    (.resources // {} | to_entries[]?
      | walk_methods($prefix + .key + ".")[]
    )
  ];

walk_methods("")
| flatten
| sort_by(.operation)
' "$DISCOVERY_FILE" > "$OUT_DIR/methods.json"

jq '
[
  (.schemas // {} | to_entries[]?
    | {
        name: .key,
        description: .value.description,
        required: (.value.required // []),
        properties: (
          (.value.properties // {})
          | to_entries
          | map({
              name: .key,
              type: (.value.type // "object"),
              format: (.value.format // null),
              enum: (.value.enum // []),
              itemsType: (.value.items.type // null),
              ref: (.value["$ref"] // null),
              description: .value.description
            })
        )
      }
  )
]
| flatten
| sort_by(.name)
' "$DISCOVERY_FILE" > "$OUT_DIR/schemas.json"

jq '
def count_methods:
  ((.methods // {} | keys | length)
    + ((.resources // {})
      | to_entries
      | map(.value | count_methods)
      | add // 0));

{
  generated_at_utc: $generated_at_utc,
  discovery_url: $discovery_url,
  discovery_revision: .revision,
  discovery_version: .version,
  schema_sha256: $schema_sha256,
  method_count: (
    (.resources // {})
    | to_entries
    | map(.value | count_methods)
    | add // 0
  ),
  auth_scopes: ((.auth.oauth2.scopes // {}) | keys | sort)
}
' \
  --arg generated_at_utc "$NOW_UTC" \
  --arg discovery_url "$DISCOVERY_URL" \
  --arg schema_sha256 "$SCHEMA_SHA" \
  "$DISCOVERY_FILE" > "$OUT_DIR/schema-metadata.json"

# Semantic surface used by OpenCraw email integration.
jq '
{
  label_type_enum: (.schemas.Label.properties.type.enum // []),
  message_list_visibility_enum: (.schemas.Label.properties.messageListVisibility.enum // []),
  label_list_visibility_enum: (.schemas.Label.properties.labelListVisibility.enum // []),
  default_query_examples: [
    "in:inbox is:unread",
    "newer_than:7d",
    "label:important"
  ]
}
' "$DISCOVERY_FILE" > "$OUT_DIR/semantic-types.json"

cat > "$OUT_DIR/README.md" <<EOF
# Gmail API Contract Snapshot

Generated (UTC): $NOW_UTC  
Discovery URL: \`$DISCOVERY_URL\`  
Schema SHA256: \`$SCHEMA_SHA\`

## Files

- \`discovery-rest-v1.json\`: full Gmail Discovery REST contract payload.
- \`schema-metadata.json\`: snapshot metadata (revision/version/hash/scope list).
- \`methods.json\`: flattened method contract inventory (path + HTTP method + params + scopes).
- \`schemas.json\`: schema/type contract inventory.
- \`semantic-types.json\`: semantic enums relevant to label/state handling.

## Regenerate

\`\`\`bash
scripts/fetch-gmail-contracts.sh
\`\`\`
EOF

echo "Gmail contracts written to $OUT_DIR"
