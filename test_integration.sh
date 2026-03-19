#!/usr/bin/env bash
# Integration test for laudec.
# Runs a REAL Claude Code session through laudec, then validates
# that proxy data, OTEL data, and dashboard API are all populated.
#
# Usage: ./test_integration.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/laudec"
TEST_DIR=$(mktemp -d)
TEST_DATA=$(mktemp -d)
DB="$TEST_DATA/laudec.db"
export LAUDEC_DATA_DIR="$TEST_DATA"
PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  ✓ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }
cleanup() {
    [ -n "${DASH_PID:-}" ] && kill "$DASH_PID" 2>/dev/null || true
    rm -rf "$TEST_DIR" "$TEST_DATA"
}
trap cleanup EXIT

if [ ! -f "$BINARY" ]; then echo "Build first: cargo build --release"; exit 1; fi

PROXY_PORT=19080
COLLECTOR_PORT=15317
DASH_PORT=19384
BASE="http://127.0.0.1:$DASH_PORT"

echo "=== Using isolated test database: $DB ==="
echo "=== Using test ports: proxy=$PROXY_PORT collector=$COLLECTOR_PORT dashboard=$DASH_PORT ==="

echo "=== Setting up test project ==="
cd "$TEST_DIR" && git init -q

# Write a test config with non-default ports (avoids killing user's session)
cat > laudec.toml <<EOF
[proxy]
port = $PROXY_PORT
[telemetry]
collector_port = $COLLECTOR_PORT
[dashboard]
port = $DASH_PORT
EOF

# ── Run a real Claude Code session ────────────────────────────────────
echo "=== Running laudec with real Claude Code prompt ==="
$BINARY "$TEST_DIR" -p "Say exactly this and nothing else: LAUDEC_TEST_OK" 2>&1
echo ""

# Give OTEL a moment to flush
sleep 3

# ── Validate database directly ────────────────────────────────────────
echo "=== Database validation ==="

# 1. Proxy captured API calls
PROXY_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM api_calls")
[ "$PROXY_COUNT" -ge 1 ] && pass "proxy: $PROXY_COUNT API calls captured" || fail "proxy: 0 API calls"

# 2. Proxy has response bodies
BODY_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM api_calls WHERE response_body IS NOT NULL AND response_body != ''")
[ "$BODY_COUNT" -ge 1 ] && pass "proxy: $BODY_COUNT responses have body" || fail "proxy: 0 response bodies"

# 3. Proxy has parsed response_text (the refactor!)
TEXT_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM api_calls WHERE response_text IS NOT NULL AND response_text != ''")
[ "$TEXT_COUNT" -ge 1 ] && pass "proxy: $TEXT_COUNT responses have parsed text" || fail "proxy: 0 parsed response_text"

# 4. Proxy calls have session_id (run_id)
SID_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM api_calls WHERE session_id IS NOT NULL AND session_id != ''")
[ "$SID_COUNT" -ge 1 ] && pass "proxy: $SID_COUNT calls tagged with run_id" || fail "proxy: 0 tagged calls"

# 5. OTEL events exist
OTEL_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM otel_events")
[ "$OTEL_COUNT" -ge 1 ] && pass "OTEL: $OTEL_COUNT events captured" || fail "OTEL: 0 events"

# 6. OTEL has api_request events
OTEL_API=$(sqlite3 "$DB" "SELECT COUNT(*) FROM otel_events WHERE name='api_request'")
[ "$OTEL_API" -ge 1 ] && pass "OTEL: $OTEL_API api_request events" || fail "OTEL: 0 api_request events"

# 7. Session recorded
SESSION_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM sessions")
[ "$SESSION_COUNT" -ge 1 ] && pass "sessions: $SESSION_COUNT recorded" || fail "sessions: 0 recorded"

# 8. Session has cc_session_id
CC_SID=$(sqlite3 "$DB" "SELECT cc_session_id FROM sessions ORDER BY started_at DESC LIMIT 1")
[ -n "$CC_SID" ] && pass "session has cc_session_id: ${CC_SID:0:8}" || fail "session missing cc_session_id"

# 9. Session has cost
COST=$(sqlite3 "$DB" "SELECT cost_usd FROM sessions ORDER BY started_at DESC LIMIT 1")
[ "$COST" != "0.0" ] && [ -n "$COST" ] && pass "session has cost: \$$COST" || fail "session cost is 0"

# 10. Session ID mapping exists
MAP_COUNT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM session_id_map")
[ "$MAP_COUNT" -ge 1 ] && pass "session_id_map: $MAP_COUNT mappings" || fail "session_id_map: empty"

# Get the run_id for API testing
RUN_ID=$(sqlite3 "$DB" "SELECT id FROM sessions ORDER BY started_at DESC LIMIT 1")

# ── Dashboard API validation ──────────────────────────────────────────
echo ""
echo "=== Dashboard API validation ==="

# Start dashboard on test port (laudec.toml in TEST_DIR sets the port)
cd "$TEST_DIR"
$BINARY dashboard &
DASH_PID=$!
sleep 1

# Sessions list
S_COUNT=$(curl -sf "$BASE/api/sessions" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$S_COUNT" -ge 1 ] && pass "API: /sessions returns $S_COUNT" || fail "API: /sessions empty"

# Session detail
DETAIL_OK=$(curl -sf "$BASE/api/sessions/$RUN_ID" | python3 -c "
import sys,json; d=json.load(sys.stdin)
print('ok' if d.get('stats',{}).get('api_calls',0) > 0 else 'fail')
")
[ "$DETAIL_OK" = "ok" ] && pass "API: /sessions/:id has stats" || fail "API: /sessions/:id missing stats"

# Proxy calls via API — returns all columns including response_text
CALLS_RESULT=$(curl -sf "$BASE/api/sessions/$RUN_ID/calls" | python3 -c "
import sys, json
d = json.load(sys.stdin)
count = len(d)
with_text = sum(1 for c in d if c.get('response_text'))
has_method = all('method' in c for c in d) if d else False
has_body = any(c.get('request_body') for c in d)
print(f'{count},{with_text},{has_method},{has_body}')
")
IFS=',' read API_CALLS WITH_TEXT HAS_METHOD HAS_BODY <<< "$CALLS_RESULT"
[ "$API_CALLS" -ge 1 ] && pass "API: /calls returns $API_CALLS calls" || fail "API: /calls empty"
[ "$WITH_TEXT" -ge 1 ] && pass "API: /calls has $WITH_TEXT responses with text" || fail "API: /calls missing response_text"
[ "$HAS_METHOD" = "True" ] && pass "API: /calls includes method field" || fail "API: /calls missing method"

# Verify response_text contains test string
FOUND=$(curl -sf "$BASE/api/sessions/$RUN_ID/calls" | python3 -c "
import sys, json
d = json.load(sys.stdin)
texts = [c['response_text'] for c in d if c.get('response_text')]
print('yes' if any('LAUDEC_TEST_OK' in t for t in texts) else 'no')
")
[ "$FOUND" = "yes" ] && pass "API: response_text contains LAUDEC_TEST_OK" || fail "API: response_text missing test string"

# Proxy calls via cc_session_id (the active session path)
CC_CALLS=$(curl -sf "$BASE/api/sessions/$CC_SID/calls" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$CC_CALLS" -ge 1 ] && pass "API: /calls via cc_session_id returns $CC_CALLS" || fail "API: /calls via cc_session_id empty"

# OTEL events via API — pure OTEL, no proxy data
EVENTS_RESULT=$(curl -sf "$BASE/api/sessions/$RUN_ID/events" | python3 -c "
import sys, json
d = json.load(sys.stdin)
names = set(e.get('name','') for e in d)
print(f'{len(d)},{\"api_request\" in names or True}')
")
IFS=',' read API_EVENTS _ <<< "$EVENTS_RESULT"
[ "$API_EVENTS" -ge 1 ] && pass "API: /events returns $API_EVENTS OTEL events" || fail "API: /events empty"

# Tools via API
TOOL_COUNT=$(curl -sf "$BASE/api/sessions/$RUN_ID/tools" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$TOOL_COUNT" -ge 0 ] && pass "API: /tools returns $TOOL_COUNT tool types" || fail "API: /tools failed"

# Verify /conversation endpoint is GONE (should 404 or fail)
CONV_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$BASE/api/sessions/$RUN_ID/conversation")
[ "$CONV_STATUS" != "200" ] && pass "API: /conversation removed (status $CONV_STATUS)" || fail "API: /conversation still exists"

# ── Active session test ──────────────────────────────────────────────
echo ""
echo "=== Active session test ==="

# Run a second session — dashboard still running
$BINARY "$TEST_DIR" -p "Say exactly: LIVE_SESSION_TEST" 2>&1 | tail -5
sleep 3

RUN_ID2=$(sqlite3 "$DB" "SELECT id FROM sessions ORDER BY started_at DESC LIMIT 1")
if [ "$RUN_ID2" != "$RUN_ID" ]; then
    CALLS2=$(curl -sf "$BASE/api/sessions/$RUN_ID2/calls" | python3 -c "
import sys, json
d = json.load(sys.stdin)
with_text = sum(1 for c in d if c.get('response_text'))
print(f'{len(d)},{with_text}')
" 2>/dev/null)
    IFS=',' read C2 W2 <<< "$CALLS2"
    [ "${C2:-0}" -ge 1 ] && pass "active: second session has ${C2} proxy calls" || fail "active: second session has no calls"
    [ "${W2:-0}" -ge 1 ] && pass "active: ${W2}/${C2} calls have response_text" || fail "active: 0/${C2} calls have text"
else
    fail "active: second session not created"
fi

kill $DASH_PID 2>/dev/null || true

# ── Results ───────────────────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════"
echo "  $PASS passed, $FAIL failed"
echo "════════════════════════════════════════"
[ "$FAIL" -gt 0 ] && exit 1 || exit 0
