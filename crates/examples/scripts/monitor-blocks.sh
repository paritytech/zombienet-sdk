#!/usr/bin/env bash
# Monitor block production for a Substrate node
# Usage: monitor-blocks.sh <ws_url> [block_count]

set -euo pipefail

WS_URL="${1:-ws://127.0.0.1:9944}"
BLOCK_COUNT="${2:-3}"

echo "🔍 Monitoring blocks at: $WS_URL" >&2
echo "📊 Will check $BLOCK_COUNT blocks" >&2
echo "⏳ Waiting for node to be ready..." >&2

# Wait for node to start
sleep 10

# Convert ws:// to http:// for RPC calls
HTTP_URL="${WS_URL/ws:/http:}"
HTTP_URL="${HTTP_URL/\/ws/}"

echo "" >&2

LAST_BLOCK=-1
COUNT=0

# Keep polling until we see the requested number of unique finalized blocks
while [ "$COUNT" -lt "$BLOCK_COUNT" ]; do
    # Get the finalized head hash
    FINALIZED_HASH=$(curl -s -H "Content-Type: application/json" \
        -d '{"id":1, "jsonrpc":"2.0", "method": "chain_getFinalizedHead"}' \
        "$HTTP_URL" 2>/dev/null | grep -o '"result":"[^"]*"' | cut -d'"' -f4)

    if [ -n "$FINALIZED_HASH" ] && [ "$FINALIZED_HASH" != "null" ]; then
        # Get the header for this finalized block
        BLOCK=$(curl -s -H "Content-Type: application/json" \
            -d "{\"id\":1, \"jsonrpc\":\"2.0\", \"method\": \"chain_getHeader\", \"params\":[\"$FINALIZED_HASH\"]}" \
            "$HTTP_URL" 2>/dev/null | grep -o '"number":"[^"]*"' | cut -d'"' -f4 || echo "0x0")

        # Convert hex to decimal
        BLOCK_NUM=$((BLOCK))

        # Only print if this is a new block
        if [ "$BLOCK_NUM" -gt "$LAST_BLOCK" ]; then
            echo "Block #$BLOCK_NUM" >&2
            LAST_BLOCK=$BLOCK_NUM
            COUNT=$((COUNT + 1))
        fi
    fi

    sleep 2
done

echo "" >&2
echo "✅ Monitoring complete. Network is producing blocks!" >&2
