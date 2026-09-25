---
name: desktop-app
description: Drive the desktop (Tauri) app for realistic end-to-end tests of GUI features and UI (launch with the embedded WebDriver server, puppeteer the UI, read backend logs). Use whenever a change needs verification in the desktop GUI.
---

# Desktop app testing

Drive the real desktop app — real WKWebView, real Rust backend — through a W3C WebDriver server embedded in the app (`tauri-plugin-webdriver`, compiled only with `--features automation`). All interaction is synthetic: clicks are DOM events dispatched inside the webview, screenshots are WKWebView snapshots. It never moves the user's mouse cursor, never needs the window focused, and works with the window in the background — the user keeps using their machine while you test.

Never ship the `automation` feature in a release build: port 4445 grants arbitrary JS execution inside the wallet webview.

## Delegate to a subagent

WebDriver interaction is far too context-heavy for the main agent (screenshots, HTML dumps, log greps). ALWAYS delegate the routine interaction loop — navigation, element finding, clicking, log greps — to a subagent (Agent tool, opus model, low reasoning effort). Give it a concrete goal and the procedure below; have it return only a compact report (what it did, what it observed, verbatim error lines if any).

Exception: visual judgement stays with the main agent. For any decision based on how the UI actually looks (what is broken, how something renders), have the subagent save a screenshot and return the path, then Read the PNG yourself — and do this at least once per session regardless. Don't take the subagent's verbal description on faith for anything load-bearing.

## Launch

- If the production eigenwallet app is running, the dev instance exits immediately (single-instance plugin, same app identifier). Ask the user to close it first — never kill their wallet yourself.
- Frontend dev server (skip if already running): `curl -s http://localhost:1420 >/dev/null || (cd src-gui && yarn dev)` — run as a background task
- App: `cd src-tauri && cargo tauri dev --no-watch --features automation -- -- --testnet` — run as a background task; a window appears on the user's desktop, leave it alone
- WebDriver server: `http://127.0.0.1:4445` — ready when `curl -s http://127.0.0.1:4445/status` succeeds

## Interaction procedure

Plain W3C WebDriver over HTTP; `jq` is available. Base URL below: `WD=http://127.0.0.1:4445`.

1. Create one session and reuse it:
   `SID=$(curl -s -XPOST $WD/session -H 'Content-Type: application/json' -d '{"capabilities":{}}' | jq -r .value.sessionId)`
2. ALWAYS look at the screen visually first: `curl -s $WD/session/$SID/screenshot | jq -r .value | base64 -d > <scratchpad>/screen.png` and Read the image. Never interact blind — confirm what state the app is actually in before every action sequence, and after any action whose effect you aren't sure about. The PNG is at retina scale (2× the logical window size, e.g. 1600×1400 for the 800×700 window).
3. Find elements by selector, never by estimating coordinates. Strategies: `css selector`, `xpath`, `link text`. For buttons by visible text use xpath:
   `EID=$(curl -s -XPOST $WD/session/$SID/element -H 'Content-Type: application/json' -d '{"using":"xpath","value":"//button[contains(., \"Continue\")]"}' | jq -r '.value["element-6066-11e4-a52e-4f735466cecf"]')`
   - Match DOM text, not what the screenshot shows: MUI renders buttons ALL-CAPS via CSS `text-transform`, so the button that displays "CONTINUE" has DOM text "Continue" (and `/element/$EID/text` returns "Continue").
   - Clickable things are often not `<button>` (e.g. selection cards). `//*[contains(text(), \"Create new wallet\")]` finds the text's element; clicking it works because the synthetic click bubbles up to the React handler.
4. Interact:
   - Click: `curl -s -XPOST $WD/session/$SID/element/$EID/click -H 'Content-Type: application/json' -d '{}'`
   - Type: `curl -s -XPOST $WD/session/$SID/element/$EID/value -H 'Content-Type: application/json' -d '{"text":"foo"}'`
   - Read text: `curl -s $WD/session/$SID/element/$EID/text | jq -r .value`
5. For waits and state dumps run JS directly:
   `curl -s -XPOST $WD/session/$SID/execute/sync -H 'Content-Type: application/json' -d '{"script":"return document.body.innerText","args":[]}'`
6. Element refs go stale when React re-renders — re-find after every UI transition, don't cache them.
7. When done: `curl -s -XDELETE $WD/session/$SID`

## Limits

The webview is all you can see and touch. Native surfaces — dialogs from `tauri-plugin-dialog`, file pickers, menus, window chrome — are invisible to snapshots and unreachable by clicks. If a flow dead-ends in a native dialog, report that instead of clicking blind.

## Verification

- Backend (Rust) logs, JSON lines: `~/Library/Application Support/xmr-btc-swap/cli/testnet/logs/swap-all.log` (mainnet: `.../cli/mainnet/logs/`)
- The log file is shared across app runs — filter by this run's timestamps before attributing errors to your actions. Peer-connection WARNs (`Outgoing connection error to peer`) at startup are normal network noise, not test failures.
- App stdout: the output file of the app background task. Backend→frontend approval flows show up here and in the log as `Emitting approval event` (e.g. the wallet-setup `SeedSelection` request), useful for confirming what the UI is waiting on.
- A test passes only if both the screen shows the expected state AND the logs contain no new errors.
