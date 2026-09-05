#!/usr/bin/env bash
# Guard: CLAUDE.md must stay small (project rule: <= 1000 tokens).
# Tokenizer-free, conservative estimate: max(chars/4, words*1.3), rounded up.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
path="${1:-$here/../CLAUDE.md}"
max="${2:-1000}"
[ -f "$path" ] || { echo "CLAUDE.md not found at $path" >&2; exit 2; }
chars=$(wc -m < "$path" | tr -d ' ')
words=$(wc -w < "$path" | tr -d ' ')
est=$(awk -v c="$chars" -v w="$words" 'BEGIN{ a=c/4.0; b=w*1.3; m=(a>b?a:b); printf("%d", (m==int(m))?m:int(m)+1) }')
if [ "$est" -le "$max" ]; then bar=OK; else bar=OVER; fi
echo "CLAUDE.md: ~${est} tokens (chars=${chars}, words=${words}) cap=${max} -> ${bar}"
if [ "$est" -gt "$max" ]; then
  echo "CLAUDE.md is ~${est} tokens, over the ${max} cap. Trim it before committing." >&2
  exit 1
fi
