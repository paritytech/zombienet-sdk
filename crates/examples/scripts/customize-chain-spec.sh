#!/bin/bash
# Read chain-spec JSON from stdin, modify it and write result to stdout.

set -euo pipefail

echo "🔧 Post-processing chain-spec (stdin->stdout)" >&2

# Alice's public key (SS58)
ALICE_ACCOUNT="5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
# Alice hex (with and without 0x) - fallback when accounts stored as hex
ALICE_HEX="0xd43593c715fdd31c61141abd04a99fd6822c8558"
ALICE_HEX_NO0X="d43593c715fdd31c61141abd04a99fd6822c8558"
NEW_BALANCE="5000000000000000000"

echo "📝 Modifying Alice's balance to ${NEW_BALANCE}..." >&2

JQ_FILTER=$(cat <<'JQ'
  def walk(f): . as $in |
    if type == "object" then
      reduce keys[] as $k ({}; . + { ($k): ($in[$k] | walk(f)) }) | f
    elif type == "array" then
      map(walk(f)) | f
    else
      f
    end;

  walk(
    if type=="array" and (.[0]|type=="string") and (.[0]==$alice or .[0]==$alice_hex or .[0]==$alice_hex_no0x) then
      [.[0], ($balance | tonumber)]
    else
      .
    end
  )
JQ
)

run_jq() {
  jq --arg alice "$ALICE_ACCOUNT" --arg alice_hex "$ALICE_HEX" --arg alice_hex_no0x "$ALICE_HEX_NO0X" --arg balance "$NEW_BALANCE" "$JQ_FILTER"
}

# Read from stdin and output modified JSON to stdout
if ! run_jq; then
  echo "❌ Failed to customize chain-spec" >&2
  exit 1
fi

exit 0
