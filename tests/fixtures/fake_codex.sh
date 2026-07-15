#!/usr/bin/env bash
set -euo pipefail
scenario="${FAKE_SCENARIO:-success}"
while IFS= read -r line; do
  if [[ "$line" == *'"method":"initialize"'* ]]; then
    printf '%s\n' '{"id":1,"result":{"userAgent":"fake/0.1","codexHome":"/tmp/fake","platformFamily":"unix","platformOs":"linux"}}'
  elif [[ "$line" == *'"method":"account/rateLimits/read"'* ]]; then
    case "$scenario" in
      success)
        printf '%s\n' '{"method":"account/rateLimits/updated","params":{"rateLimits":{}}}'
        printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":28,"resetsAt":1784109000,"windowDurationMins":300},"secondary":{"usedPercent":59,"resetsAt":1784682000,"windowDurationMins":10080}}}}'
        ;;
      signed-out) printf '%s\n' '{"id":2,"error":{"code":-32603,"message":"not logged in"}}' ;;
      malformed) printf '%s\n' '{broken-json' ;;
      timeout) sleep 2 ;;
      exit) exit 7 ;;
    esac
  fi
done
