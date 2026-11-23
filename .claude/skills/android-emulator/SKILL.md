---
name: android-emulator
description: Drive the Android emulator for realistic end-to-end tests of the app's Android features and UI (install APK, puppeteer the UI, read backend logs). Use whenever a change needs verification on Android.
---

# Android emulator testing

Use the emulator for realistic end-to-end tests of Android features and UI — install the current APK, drive the app like a user would, and verify behaviour through the screen and the backend logs.

## Delegate to a subagent

Emulator interaction is far too context-heavy for the main agent (screenshots, XML dumps, log greps). ALWAYS delegate the routine interaction loop — navigation, coordinate gathering via uiautomator, tapping, log greps — to a subagent (Agent tool, opus model, low reasoning effort). Give it a concrete goal and the procedure below; have it return only a compact report (what it did, what it observed, verbatim error lines if any). The main agent never runs uiautomator/logcat itself.

Exception: visual judgement stays with the main agent. For any decision based on how the UI actually looks (what is broken, how something renders), have the subagent save a screenshot (`adb exec-out screencap -p > <scratchpad>/screen.png`) and return the path, then Read the PNG yourself — and do this at least once per session regardless. Don't take the subagent's verbal description on faith for anything load-bearing.

## Environment

- SDK root: `~/Android/Sdk` on Linux, `~/Library/Android/sdk` on macOS — below `$SDK` means that root
- adb: `$SDK/platform-tools/adb`, emulator binary: `$SDK/emulator/emulator`
- AVD: `eigen` (shows up as `emulator-5554`), screen 1080x2400
- Start if not running: `$SDK/emulator/emulator -avd eigen &` then `adb wait-for-device`
- App: package `net.unstoppableswap.gui`, activity `.MainActivity`
- APK: `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`
- Install: `adb install -r <apk>` — launch: `adb shell am start -n net.unstoppableswap.gui/.MainActivity`
- Fresh app state: `adb shell pm clear net.unstoppableswap.gui`

## Interaction procedure

1. ALWAYS look at the screen visually first: `adb exec-out screencap -p > <scratchpad>/screen.png` and Read the image. Never interact blind — confirm what state the app is actually in before every action sequence, and after any action whose effect you aren't sure about.
2. To tap something, do NOT estimate pixel coordinates from the screenshot. Get exact coordinates from the view structure: run `adb shell uiautomator dump` then `adb exec-out cat /sdcard/window_dump.xml`. The Tauri WebView exposes its full accessibility tree, so every element appears with its text/content-desc and exact `bounds="[x1,y1][x2,y2]"`. Grep the XML for the element's text, compute the center of its bounds, and tap it: `adb shell input tap <cx> <cy>`.
3. Text entry: `adb shell input text 'foo%sbar'` (`%s` = space); keys via `adb shell input keyevent <code>` (4 = back, 66 = enter).
4. Re-dump after every UI transition — bounds go stale.

## Verification

- Backend (Rust) logs: `adb logcat -d | grep RustStdoutStderr` (tracing JSON)
- Native crashes: `adb logcat -d -b crash`
- A test passes only if both the screen shows the expected state AND the logs contain no new errors/crashes.
