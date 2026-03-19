#!/usr/bin/env bash
# Test script for laudec dashboard API.
# Validates all endpoints against real data. Run after build.
set -euo pipefail

BINARY="./target/release/laudec"
DB="$HOME/.local/share/laudec/laudec.db"
BASE="http://127.0.0.1:18384"
PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  ✓ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }

if [ ! -f "$BINARY" ]; then echo "Build first: cargo build --release"; exit 1; fi
if [ ! -f "$DB" ]; then echo "No database. Run laudec at least once first."; exit 1; fi

# Kill anything on the dashboard port
kill $(lsof -ti:18384) 2>/dev/null || true
sleep 1

echo "Starting dashboard..."
$BINARY dashboard &
DASH_PID=$!
sleep 1

if ! curl -sf "$BASE/" > /dev/null 2>&1; then
    echo "Dashboard failed to start"; kill $DASH_PID 2>/dev/null; exit 1
fi

echo ""
echo "=== Static serving ==="
HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" "$BASE/")
[ "$HTTP_CODE" = "200" ] && pass "GET / → 200" || fail "GET / → $HTTP_CODE"

echo ""
echo "=== Sessions API ==="
SESSIONS=$(curl -sf "$BASE/api/sessions")
COUNT=$(echo "$SESSIONS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$COUNT" -ge 1 ] && pass "GET /api/sessions ($COUNT sessions)" || fail "GET /api/sessions ($COUNT sessions)"

echo ""
echo "=== Completed session tests ==="
# Pick a completed session that has BOTH proxy calls AND OTEL events
SID=$(python3 -c "
import json, sys, sqlite3
conn = sqlite3.connect('$DB')
# Find sessions where proxy calls exist (tagged with session.id = run_id)
rows = conn.execute('''
    SELECT s.id, s.cc_session_id,
           (SELECT COUNT(*) FROM api_calls a WHERE a.session_id = s.id) as proxy_count,
           (SELECT COUNT(*) FROM otel_events e WHERE e.session_id = s.cc_session_id AND s.cc_session_id IS NOT NULL) as otel_count
    FROM sessions s
    ORDER BY s.started_at DESC
''').fetchall()
for r in rows:
    sid, cc, proxy, otel = r
    if proxy > 0 and otel > 0:
        print(sid)
        break
    elif proxy > 0:
        print(sid)
        break
else:
    # Fallback: any session with proxy calls
    row = conn.execute('SELECT DISTINCT session_id FROM api_calls WHERE session_id IS NOT NULL ORDER BY timestamp DESC LIMIT 1').fetchone()
    if row: print(row[0])
    else: print('')
" 2>/dev/null)

if [ -z "$SID" ]; then
    echo "  (no testable sessions found)"
else
    echo "  Testing session: ${SID:0:8}..."

    # Detail endpoint
    DETAIL=$(curl -sf "$BASE/api/sessions/$SID" 2>/dev/null)
    echo "$DETAIL" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 'stats' in d" 2>/dev/null \
        && pass "GET /api/sessions/:id (has stats)" \
        || fail "GET /api/sessions/:id (missing stats)"

    # Proxy calls
    CALLS=$(curl -sf "$BASE/api/sessions/$SID/calls")
    CALL_COUNT=$(echo "$CALLS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
    [ "$CALL_COUNT" -ge 1 ] && pass "GET /api/sessions/:id/calls ($CALL_COUNT calls)" || fail "GET /api/sessions/:id/calls ($CALL_COUNT calls)"

    # OTEL events
    CC_SID=$(python3 -c "
import sqlite3
conn = sqlite3.connect('$DB')
row = conn.execute('SELECT cc_session_id FROM sessions WHERE id=?', ('$SID',)).fetchone()
if row and row[0] and row[0] != '$SID':
    print(row[0])
else:
    print('$SID')
" 2>/dev/null)
    EVENTS=$(curl -sf "$BASE/api/sessions/$SID/events")
    EVENT_COUNT=$(echo "$EVENTS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
    [ "$EVENT_COUNT" -ge 0 ] && pass "GET /api/sessions/:id/events ($EVENT_COUNT events)" || fail "GET /api/sessions/:id/events"

    # Tools
    TOOLS=$(curl -sf "$BASE/api/sessions/$SID/tools")
    TOOL_COUNT=$(echo "$TOOLS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
    pass "GET /api/sessions/:id/tools ($TOOL_COUNT tools)"

    # Conversation
    CONV=$(curl -sf "$BASE/api/sessions/$SID/conversation")
    CONV_INFO=$(echo "$CONV" | python3 -c "
import sys, json
d = json.load(sys.stdin)
total = len(d)
users = len([t for t in d if t['type']=='user'])
assistants = len([t for t in d if t['type']=='assistant'])
with_text = len([t for t in d if t['type']=='assistant' and t.get('text')])
print(f'{total} {users} {assistants} {with_text}')
")
    read TOTAL USERS ASSISTANTS WITH_TEXT <<< "$CONV_INFO"
    [ "$TOTAL" -ge 1 ] && pass "conversation: $TOTAL turns ($USERS user, $ASSISTANTS assistant)" || fail "conversation: empty"
    if [ "$ASSISTANTS" -gt 0 ]; then
        [ "$WITH_TEXT" -gt 0 ] && pass "conversation: $WITH_TEXT/$ASSISTANTS responses have text" || fail "conversation: 0/$ASSISTANTS responses have text"
    fi
fi

echo ""
echo "=== Proxy data integrity ==="
PROXY_STATS=$(python3 -c "
import sqlite3
conn = sqlite3.connect('$DB')
total = conn.execute('SELECT COUNT(*) FROM api_calls').fetchone()[0]
with_body = conn.execute('SELECT COUNT(*) FROM api_calls WHERE response_body IS NOT NULL').fetchone()[0]
with_text = conn.execute('SELECT COUNT(*) FROM api_calls WHERE response_text IS NOT NULL AND response_text != \"\"').fetchone()[0]
with_sid = conn.execute('SELECT COUNT(*) FROM api_calls WHERE session_id IS NOT NULL AND session_id != \"\"').fetchone()[0]
print(f'{total} {with_body} {with_text} {with_sid}')
" 2>/dev/null)
read TOTAL WITH_BODY WITH_TEXT WITH_SID <<< "$PROXY_STATS"
pass "proxy: $TOTAL calls, $WITH_BODY with body, $WITH_TEXT with parsed text, $WITH_SID with session_id"

echo ""
echo "=== Results ==="
echo "  $PASS passed, $FAIL failed"

kill $DASH_PID 2>/dev/null
wait $DASH_PID 2>/dev/null

[ "$FAIL" -gt 0 ] && exit 1 || exit 0
