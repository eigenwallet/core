#!/usr/bin/env bash
# Regenerate the .keys + .json test fixtures for monero-wallet2-adapter.
#
# Requirements:
#   - monero-wallet-rpc on PATH (tested with v0.18.4)
#   - python3
#
# Fixtures are committed; only re-run if you deliberately want to replace
# them. They cover three password shapes (empty, ASCII, UTF-8 + emoji).

set -euo pipefail

FIXTURE_DIR=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d -t m2a-fixtures-XXXX)
trap 'rm -rf "$WORK"' EXIT

PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')

echo "[*] launching monero-wallet-rpc on 127.0.0.1:$PORT (offline)"
monero-wallet-rpc \
  --offline \
  --disable-rpc-login \
  --wallet-dir "$WORK" \
  --rpc-bind-ip 127.0.0.1 \
  --rpc-bind-port "$PORT" \
  --log-level 0 \
  --log-file "$WORK/rpc.log" &
RPC_PID=$!
trap 'kill "$RPC_PID" 2>/dev/null || true; rm -rf "$WORK"' EXIT

# Wait for RPC readiness.
for _ in $(seq 1 30); do
  if curl -fsS -o /dev/null -X POST "http://127.0.0.1:$PORT/json_rpc" \
       -H 'Content-Type: application/json' \
       -d '{"jsonrpc":"2.0","id":"0","method":"get_version"}'; then
    break
  fi
  sleep 0.3
done

python3 - "$WORK" "$FIXTURE_DIR" "$PORT" <<'PY'
import json, shutil, sys
from pathlib import Path
from urllib.request import Request, urlopen

work, fixtures, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
RPC = f"http://127.0.0.1:{port}/json_rpc"


def rpc(method, params=None):
    body = json.dumps(
        {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}
    ).encode()
    req = Request(RPC, data=body, headers={"Content-Type": "application/json"})
    with urlopen(req, timeout=30) as r:
        resp = json.loads(r.read())
    if "error" in resp:
        raise RuntimeError(f"{method}: {resp['error']}")
    return resp["result"]


def make(name, password):
    try:
        rpc("close_wallet")
    except RuntimeError:
        pass
    rpc("create_wallet", {"filename": name, "password": password, "language": "English"})
    spend = rpc("query_key", {"key_type": "spend_key"})["key"]
    view = rpc("query_key", {"key_type": "view_key"})["key"]
    address = rpc("get_address")["address"]
    rpc("close_wallet")
    return {
        "password": password,
        "spend_secret_key": spend,
        "view_secret_key": view,
        "primary_address": address,
    }


cases = [
    ("wallet_empty", ""),
    ("wallet_short", "hunter2"),
    ("wallet_long", "correct-horse-battery-staple-42"),
]

for name, password in cases:
    print(f"    {name}: password len {len(password)}")
    meta = make(name, password)
    shutil.copyfile(Path(work) / f"{name}.keys", Path(fixtures) / f"{name}.keys")
    Path(fixtures, f"{name}.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n"
    )
PY

echo "[*] stopping monero-wallet-rpc"
curl -fsS -o /dev/null -X POST "http://127.0.0.1:$PORT/json_rpc" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":"0","method":"stop_wallet"}' || true
wait "$RPC_PID" 2>/dev/null || true
echo "done. fixtures written to $FIXTURE_DIR"
