#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

[[ -f .env ]] && { set -a; source .env; set +a; }

LINEAR_GRAPHQL_URL="${OPENCRAW_LINEAR_GRAPHQL_URL:-https://api.linear.app/graphql}"
LINEAR_API_KEY="${OPENCRAW_LINEAR_API_KEY:-}"

require_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $name" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd jq

if [[ -z "$LINEAR_API_KEY" ]]; then
  echo "ERROR: missing OPENCRAW_LINEAR_API_KEY (in env or .env)." >&2
  exit 1
fi

graphql_call() {
  local query="$1"
  local variables_json="${2:-{}}"
  local payload
  payload="$(jq -cn --arg q "$query" --argjson vars "$variables_json" '{query: $q, variables: $vars}')"
  curl -fsS "$LINEAR_GRAPHQL_URL" \
    -H "Authorization: $LINEAR_API_KEY" \
    -H "Content-Type: application/json" \
    --data "$payload"
}

ensure_success() {
  local label="$1"
  local body="$2"
  if jq -e '.errors | length > 0' >/dev/null 2>&1 <<<"$body"; then
    echo "FAIL: $label returned GraphQL errors" >&2
    jq -c '.errors' <<<"$body" >&2
    return 1
  fi
  return 0
}

TYPE_REF_FRAGMENT='fragment TypeRef on __Type {
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
}'

INTROSPECT_QUERY="$TYPE_REF_FRAGMENT
query ContractIntrospection {
  queryType: __type(name: \"Query\") {
    fields {
      name
    }
  }
  mutationType: __type(name: \"Mutation\") {
    fields {
      name
      args {
        name
        type { ...TypeRef }
      }
    }
  }
  issueCreateInput: __type(name: \"IssueCreateInput\") {
    kind
    inputFields {
      name
      type { ...TypeRef }
    }
  }
  projectCreateInput: __type(name: \"ProjectCreateInput\") {
    kind
    inputFields {
      name
      type { ...TypeRef }
    }
  }
  issueUpdateInput: __type(name: \"IssueUpdateInput\") {
    kind
    inputFields {
      name
      type { ...TypeRef }
    }
  }
  commentCreateInput: __type(name: \"CommentCreateInput\") {
    kind
    inputFields {
      name
      type { ...TypeRef }
    }
  }
}"

echo "Running Linear schema contract checks against $LINEAR_GRAPHQL_URL"
INTROSPECTION_JSON="$(graphql_call "$INTROSPECT_QUERY" '{}')"
ensure_success "schema introspection" "$INTROSPECTION_JSON"

type_to_string_jq='
def type_to_string($t):
  if $t == null then ""
  elif $t.kind == "NON_NULL" then (type_to_string($t.ofType) + "!")
  elif $t.kind == "LIST" then ("[" + type_to_string($t.ofType) + "]")
  else ($t.name // "")
  end;
'

check_pass=0
check_fail=0

ok() {
  local msg="$1"
  echo "PASS: $msg"
  check_pass=$((check_pass + 1))
}

fail() {
  local msg="$1"
  echo "FAIL: $msg"
  check_fail=$((check_fail + 1))
}

expect_query_field() {
  local field="$1"
  if jq -e --arg f "$field" '.data.queryType.fields[]? | select(.name == $f)' >/dev/null <<<"$INTROSPECTION_JSON"; then
    ok "Query.$field exists"
  else
    fail "Query.$field missing"
  fi
}

expect_mutation_arg_type() {
  local field="$1"
  local arg="$2"
  local expected="$3"
  local actual
  actual="$(
    jq -r --arg f "$field" --arg a "$arg" "$type_to_string_jq
      .data.mutationType.fields[]? | select(.name == \$f)
      | .args[]? | select(.name == \$a)
      | type_to_string(.type)
    " <<<"$INTROSPECTION_JSON" | head -n 1
  )"
  if [[ -z "$actual" ]]; then
    fail "Mutation.$field arg $arg missing (expected type $expected)"
    return
  fi
  if [[ "$actual" == "$expected" ]]; then
    ok "Mutation.$field($arg: $expected)"
  else
    fail "Mutation.$field($arg) type mismatch: expected $expected, got $actual"
  fi
}

expect_input_field() {
  local alias="$1"
  local type_name="$2"
  local field="$3"
  if jq -e --arg a "$alias" --arg f "$field" '.data[$a].inputFields[]? | select(.name == $f)' >/dev/null <<<"$INTROSPECTION_JSON"; then
    ok "$type_name.$field exists"
  else
    fail "$type_name.$field missing"
  fi
}

# Query roots used by opencraw linear tool.
expect_query_field "viewer"
expect_query_field "users"
expect_query_field "teams"
expect_query_field "projects"

# Mutation signatures expected by opencraw.
expect_mutation_arg_type "issueCreate" "input" "IssueCreateInput!"
expect_mutation_arg_type "projectCreate" "input" "ProjectCreateInput!"
expect_mutation_arg_type "issueUpdate" "id" "String!"
expect_mutation_arg_type "issueUpdate" "input" "IssueUpdateInput!"
expect_mutation_arg_type "commentCreate" "input" "CommentCreateInput!"

# Input fields used by opencraw mutations.
expect_input_field "issueCreateInput" "IssueCreateInput" "title"
expect_input_field "issueCreateInput" "IssueCreateInput" "teamId"
expect_input_field "issueCreateInput" "IssueCreateInput" "description"
expect_input_field "issueCreateInput" "IssueCreateInput" "assigneeId"
expect_input_field "issueCreateInput" "IssueCreateInput" "priority"

expect_input_field "projectCreateInput" "ProjectCreateInput" "name"
if jq -e '.data.projectCreateInput.inputFields[]? | select(.name == "teamIds" or .name == "teamId")' >/dev/null <<<"$INTROSPECTION_JSON"; then
  ok "ProjectCreateInput has teamIds/teamId contract"
else
  fail "ProjectCreateInput missing both teamIds and teamId"
fi

expect_input_field "issueUpdateInput" "IssueUpdateInput" "title"
expect_input_field "issueUpdateInput" "IssueUpdateInput" "description"
expect_input_field "issueUpdateInput" "IssueUpdateInput" "assigneeId"
expect_input_field "issueUpdateInput" "IssueUpdateInput" "priority"
expect_input_field "issueUpdateInput" "IssueUpdateInput" "stateId"
expect_input_field "issueUpdateInput" "IssueUpdateInput" "projectId"

expect_input_field "commentCreateInput" "CommentCreateInput" "issueId"
expect_input_field "commentCreateInput" "CommentCreateInput" "body"

echo
echo "Linear contract summary: pass=$check_pass fail=$check_fail"
if [[ "$check_fail" -gt 0 ]]; then
  exit 1
fi

