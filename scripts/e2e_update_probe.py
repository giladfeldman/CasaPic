"""Reads what the running test copy of CasaPic reports about itself (version, updates on),
through WebView2's debugging port. Used by Test-UpdaterEndToEnd.ps1. Prints one JSON line."""
import asyncio
import json
import sys
import time
import urllib.request

import websockets

PORT = int(sys.argv[1])
TIMEOUT = float(sys.argv[2]) if len(sys.argv) > 2 else 60


def page():
    end = time.time() + TIMEOUT
    while time.time() < end:
        try:
            for p in json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2)):
                if p.get("type") == "page":
                    return p["webSocketDebuggerUrl"]
        except Exception:
            pass
        time.sleep(0.5)
    return None


async def main():
    url = page()
    if not url:
        print(json.dumps({"ok": False, "error": "no page on the debugging port"}))
        return
    expr = "window.__TAURI_INTERNALS__.invoke('update_status')"
    async with websockets.connect(url, max_size=None) as ws:
        await ws.send(json.dumps({"id": 1, "method": "Runtime.evaluate",
                                  "params": {"expression": expr, "returnByValue": True, "awaitPromise": True}}))
        while True:
            m = json.loads(await ws.recv())
            if m.get("id") == 1:
                v = m.get("result", {}).get("result", {}).get("value")
                print(json.dumps({"ok": isinstance(v, dict), "status": v}))
                return


asyncio.run(main())
