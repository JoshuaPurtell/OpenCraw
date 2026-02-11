#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

LINEAR_GRAPHQL_URL="${OPENCRAW_LINEAR_GRAPHQL_URL:-https://api.linear.app/graphql}"
LINEAR_API_KEY="${OPENCRAW_LINEAR_API_KEY:-}"
OUT_DIR="${1:-contracts/linear}"

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

if [[ -z "$LINEAR_API_KEY" ]]; then
  echo "ERROR: missing OPENCRAW_LINEAR_API_KEY (in env or .env)." >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

TMP_RESPONSE="$(mktemp)"
TMP_DATA="$(mktemp)"
trap 'rm -f "$TMP_RESPONSE" "$TMP_DATA"' EXIT

read -r -d '' INTROSPECTION_QUERY <<'GRAPHQL' || true
query LinearSchemaIntrospection {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      description
      fields(includeDeprecated: true) {
        name
        description
        args {
          name
          description
          defaultValue
          type { ...TypeRef }
        }
        type { ...TypeRef }
        isDeprecated
        deprecationReason
      }
      inputFields {
        name
        description
        defaultValue
        type { ...TypeRef }
      }
      interfaces { ...TypeRef }
      enumValues(includeDeprecated: true) {
        name
        description
        isDeprecated
        deprecationReason
      }
      possibleTypes { ...TypeRef }
    }
    directives {
      name
      description
      locations
      args {
        name
        description
        defaultValue
        type { ...TypeRef }
      }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType {
                kind
                name
                ofType {
                  kind
                  name
                }
              }
            }
          }
        }
      }
    }
  }
}
GRAPHQL

PAYLOAD="$(jq -cn --arg q "$INTROSPECTION_QUERY" '{query: $q}')"

echo "Fetching Linear GraphQL schema contracts from $LINEAR_GRAPHQL_URL"
curl -fsS "$LINEAR_GRAPHQL_URL" \
  -H "Authorization: $LINEAR_API_KEY" \
  -H "Content-Type: application/json" \
  --data "$PAYLOAD" > "$TMP_RESPONSE"

if jq -e '.errors and (.errors | length > 0)' >/dev/null 2>&1 < "$TMP_RESPONSE"; then
  echo "ERROR: GraphQL introspection returned errors:" >&2
  jq -c '.errors' < "$TMP_RESPONSE" >&2
  exit 1
fi

jq '.data' < "$TMP_RESPONSE" > "$TMP_DATA"

DATA_FILE="$OUT_DIR/introspection-full.json"
cp "$TMP_DATA" "$DATA_FILE"

TYPE_REF_JQ='
def type_to_string:
  if . == null then null
  elif .kind == "NON_NULL" then ((.ofType | type_to_string) + "!")
  elif .kind == "LIST" then ("[" + (.ofType | type_to_string) + "]")
  else .name
  end;
'

jq "$TYPE_REF_JQ
  .__schema as \$s
  | (\$s.queryType.name // \"\") as \$root
  | [ \$s.types[] | select(.name == \$root) | .fields[]?
      | {
          name,
          description,
          returns: (.type | type_to_string),
          args: [ .args[]? | {
            name,
            description,
            type: (.type | type_to_string),
            defaultValue
          }]
        }
    ]
  | sort_by(.name)
" < "$TMP_DATA" > "$OUT_DIR/query-fields.json"

jq "$TYPE_REF_JQ
  .__schema as \$s
  | (\$s.mutationType.name // \"\") as \$root
  | [ \$s.types[] | select(.name == \$root) | .fields[]?
      | {
          name,
          description,
          returns: (.type | type_to_string),
          args: [ .args[]? | {
            name,
            description,
            type: (.type | type_to_string),
            defaultValue
          }]
        }
    ]
  | sort_by(.name)
" < "$TMP_DATA" > "$OUT_DIR/mutation-fields.json"

jq "$TYPE_REF_JQ
  .__schema as \$s
  | (\$s.subscriptionType.name // \"\") as \$root
  | [ \$s.types[] | select(.name == \$root) | .fields[]?
      | {
          name,
          description,
          returns: (.type | type_to_string),
          args: [ .args[]? | {
            name,
            description,
            type: (.type | type_to_string),
            defaultValue
          }]
        }
    ]
  | sort_by(.name)
" < "$TMP_DATA" > "$OUT_DIR/subscription-fields.json"

jq "$TYPE_REF_JQ
  [ .__schema.types[]
    | select(.kind == \"INPUT_OBJECT\")
    | {
        name,
        description,
        fields: [
          .inputFields[]? | {
            name,
            description,
            type: (.type | type_to_string),
            defaultValue
          }
        ]
      }
  ]
  | sort_by(.name)
" < "$TMP_DATA" > "$OUT_DIR/input-objects.json"

jq '
  [ .__schema.types[]
    | select(.kind == "ENUM")
    | {
        name,
        description,
        values: [
          .enumValues[]? | {
            name,
            description,
            isDeprecated,
            deprecationReason
          }
        ]
      }
  ]
  | sort_by(.name)
' < "$TMP_DATA" > "$OUT_DIR/enum-types.json"

jq '
  [ .[] | select(.name | test("state|status"; "i")) ]
  | sort_by(.name)
' < "$OUT_DIR/enum-types.json" > "$OUT_DIR/state-related-enums.json"

jq '
  [ .__schema.types[]
    | select(.kind == "SCALAR")
    | {
        name,
        description
      }
  ]
  | sort_by(.name)
' < "$TMP_DATA" > "$OUT_DIR/scalars.json"

jq '
  [ .__schema.directives[]?
    | {
        name,
        description,
        locations,
        args
      }
  ]
  | sort_by(.name)
' < "$TMP_DATA" > "$OUT_DIR/directives.json"

SCHEMA_HASH="$(shasum -a 256 "$DATA_FILE" | awk '{print $1}')"
FETCHED_AT_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
QUERY_COUNT="$(jq 'length' "$OUT_DIR/query-fields.json")"
MUTATION_COUNT="$(jq 'length' "$OUT_DIR/mutation-fields.json")"
SUBSCRIPTION_COUNT="$(jq 'length' "$OUT_DIR/subscription-fields.json")"
INPUT_COUNT="$(jq 'length' "$OUT_DIR/input-objects.json")"
ENUM_COUNT="$(jq 'length' "$OUT_DIR/enum-types.json")"
STATE_ENUM_COUNT="$(jq 'length' "$OUT_DIR/state-related-enums.json")"
SCALAR_COUNT="$(jq 'length' "$OUT_DIR/scalars.json")"
DIRECTIVE_COUNT="$(jq 'length' "$OUT_DIR/directives.json")"

jq -n \
  --arg fetched_at_utc "$FETCHED_AT_UTC" \
  --arg graphql_url "$LINEAR_GRAPHQL_URL" \
  --arg schema_sha256 "$SCHEMA_HASH" \
  --argjson query_fields "$QUERY_COUNT" \
  --argjson mutation_fields "$MUTATION_COUNT" \
  --argjson subscription_fields "$SUBSCRIPTION_COUNT" \
  --argjson input_objects "$INPUT_COUNT" \
  --argjson enum_types "$ENUM_COUNT" \
  --argjson state_related_enums "$STATE_ENUM_COUNT" \
  --argjson scalar_types "$SCALAR_COUNT" \
  --argjson directives "$DIRECTIVE_COUNT" \
  '{
    fetched_at_utc: $fetched_at_utc,
    graphql_url: $graphql_url,
    schema_sha256: $schema_sha256,
    counts: {
      query_fields: $query_fields,
      mutation_fields: $mutation_fields,
      subscription_fields: $subscription_fields,
      input_objects: $input_objects,
      enum_types: $enum_types,
      state_related_enums: $state_related_enums,
      scalar_types: $scalar_types,
      directives: $directives
    }
  }' > "$OUT_DIR/schema-metadata.json"

STATE_ENUM_LINES="$(jq -r '
  if length == 0 then
    "- (none detected via introspection)"
  else
    .[] | "- \(.name): \(([.values[]?.name] | join(", ")))"
  end
' < "$OUT_DIR/state-related-enums.json")"

cat > "$OUT_DIR/README.md" <<EOF
# Linear API Contract Snapshot

Generated (UTC): $FETCHED_AT_UTC  
GraphQL endpoint: \`$LINEAR_GRAPHQL_URL\`  
Schema SHA256: \`$SCHEMA_HASH\`

This directory stores a reproducible snapshot of the **full Linear GraphQL introspection contract** and extracted contracts used by OpenCraw.

## Files

- \`introspection-full.json\`: full \`__schema\` introspection payload.
- \`schema-metadata.json\`: snapshot metadata and contract counts.
- \`query-fields.json\`: root query field contracts (args + return types).
- \`mutation-fields.json\`: root mutation field contracts (args + return types).
- \`subscription-fields.json\`: root subscription contracts (if exposed).
- \`input-objects.json\`: every GraphQL input object contract.
- \`enum-types.json\`: every enum type + allowed values.
- \`state-related-enums.json\`: enums with \`state\`/\`status\` in their type name.
- \`scalars.json\`: scalar type inventory.
- \`directives.json\`: directive contracts.

## Regenerate

\`\`\`bash
scripts/fetch-linear-contracts.sh
\`\`\`

## State/Status enums detected

$STATE_ENUM_LINES

## Notes

- This is the source of truth for schema-level contracts.
- Some runtime validation constraints may exist beyond introspection and are surfaced as GraphQL errors at execution time.
EOF

echo "Linear contracts written to $OUT_DIR"
