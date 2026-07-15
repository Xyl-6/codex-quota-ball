#!/usr/bin/env bash
set -euo pipefail
scenario="${FAKE_SCENARIO:-success}"
if [[ "${1:-}" == "--version" ]]; then
  if [[ "$scenario" == "version-hang" ]]; then
    exec sleep 2
  fi
  if [[ "$scenario" == "version-descendant" ]]; then
    sleep 2 &
  fi
  printf '%s\n' 'fake-codex 1.2.3'
  exit 0
fi
expected_account_id=2
expect_account_id() {
  if [[ "$1" != "$expected_account_id" ]]; then
    printf 'expected account request id %s, got %s\n' "$expected_account_id" "$1" >&2
    exit 9
  fi
  expected_account_id=$((expected_account_id + 1))
}
while IFS= read -r line; do
  id="$(sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p' <<<"$line")"
  if [[ "$line" == *'"method":"initialize"'* ]]; then
    printf '%s\n' "{\"id\":$id,\"result\":{\"userAgent\":\"fake/0.1\",\"codexHome\":\"/tmp/fake\",\"platformFamily\":\"unix\",\"platformOs\":\"linux\"}}"
  elif [[ "$line" == *'"method":"account/rateLimits/read"'* ]]; then
    expect_account_id "$id"
    case "$scenario" in
      success|usage-error|usage-incompatible)
        printf '%s\n' '{"method":"account/rateLimits/updated","params":{"rateLimits":{}}}'
        printf '%s\n' "{\"id\":$id,\"result\":{\"rateLimits\":{\"primary\":{\"usedPercent\":28,\"resetsAt\":1784109000,\"windowDurationMins\":300},\"secondary\":{\"usedPercent\":59,\"resetsAt\":1784682000,\"windowDurationMins\":10080}}}}"
        ;;
      signed-out) printf '%s\n' "{\"id\":$id,\"error\":{\"code\":-32603,\"message\":\"not logged in\"}}" ;;
      malformed) printf '%s\n' '{broken-json' ;;
      timeout) exec sleep 2 ;;
      exit) exit 7 ;;
      version-hang|version-descendant) printf '%s\n' "{\"id\":$id,\"result\":{}}" ;;
    esac
  elif [[ "$line" == *'"method":"account/usage/read"'* ]]; then
    expect_account_id "$id"
    case "$scenario" in
      usage-error)
        printf '%s\n' "{\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"account/usage/read unavailable\"}}"
        ;;
      usage-incompatible)
        printf '%s\n' "{\"id\":$id,\"result\":{\"summary\":{},\"dailyUsageBuckets\":\"invalid\"}}"
        ;;
      *)
        printf '%s\n' "{\"id\":$id,\"result\":{\"summary\":{},\"dailyUsageBuckets\":[{\"startDate\":\"2026-07-13\",\"tokens\":1200}]}}"
        ;;
    esac
  fi
done
