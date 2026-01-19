#!/bin/bash
# Example chain-spec post-processing script
#
# This script receives:
#   $1: Path to the chain-spec JSON file
#
# The script should modify the chain-spec file in place.
# Exit with 0 for success, non-zero for failure.

SPEC_PATH="$1"

echo "🔧 Post-processing chain-spec at: $SPEC_PATH"

# Alice's public key (SS58)
ALICE_ACCOUNT="5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
# Alice hex (with and without 0x) - fallback when accounts stored as hex
ALICE_HEX="0xd43593c715fdd31c61141abd04a99fd6822c8558"
ALICE_HEX_NO0X="d43593c715fdd31c61141abd04a99fd6822c8558"
NEW_BALANCE="5000000000000000000"

echo "📝 Modifying Alice's balance to ${NEW_BALANCE}..."

# Recursively walk the JSON and replace any [account, balance] arrays where the account matches
# This handles both plain and raw chain-spec JSON structures.
jq --arg alice "$ALICE_ACCOUNT" --arg alice_hex "$ALICE_HEX" --arg alice_hex_no0x "$ALICE_HEX_NO0X" --arg balance "$NEW_BALANCE" '
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
' "$SPEC_PATH" > "$SPEC_PATH.tmp"

# Check if jq succeeded
if [ $? -eq 0 ]; then
    mv "$SPEC_PATH.tmp" "$SPEC_PATH"
    echo "✅ Successfully customized chain-spec - Alice's balance updated"
    exit 0
else
    echo "❌ Failed to customize chain-spec"
    rm -f "$SPEC_PATH.tmp"
    exit 1
fi
