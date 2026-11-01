# AGENTS.md - Instructions for AI Agents and Contributors

ReShift for macOS: a lean, fast menu bar utility that converts text typed in the wrong keyboard layout.
The goal is a responsive, reliable tool: it must never hang, use little memory, start instantly and stay lightweight.

## Testing Requirements

**Tests MUST be run after every edit of `.rs` files:**

```bash
cargo test --workspace
```

### Rules

1. **After every code change**: run the unit tests before considering the task complete.
2. **No failures, no warnings**: before a commit also run `cargo fmt --all -- --check` and
   `cargo clippy --workspace --all-targets -- -D warnings`.
3. **New functionality requires tests**: any new feature, bug fix or behavioral change needs unit tests.
4. **E2E smoke test**: run `./scripts/e2e.sh` to test real conversions via simulated keyboard events.
5. **Event tap tests** (`tap::tests`): create real taps and post synthetic keys, so they are `#[ignore]`d. Run them
   on purpose with Accessibility permission: `cargo test -p reshift -- --ignored --test-threads=1`.
6. Unit tests live next to the code (`#[cfg(test)] mod tests`).

## Project Structure

- `crates/core` (`reshift-core`) - platform-neutral logic, no macOS APIs.
- `crates/app` (`reshift`) - the AppKit menu bar app: event tap, pasteboard, windows.
- Section comments `// MARK: -` are kept to make navigation easy.

## Identity and Settings

- Bundle ID `com.reshift.app` (release) and `com.reshift.app.dev` (dev builds).
- Settings are stored in UserDefaults.

## macOS Mechanics - Do Not Break

- **Threads**: the main thread (AppKit) never waits for a conversion; the pipeline runs on its own thread;
  every wait has a timeout.
- The **event tap runs on its own thread**; its callback must stay fast. It feeds the double Shift detector,
  matches the custom hotkey (and swallows it) and posts messages.
- Our own synthetic events carry a marker in `eventSourceUserData` (`0x4C535743`, "LSWC"); the tap ignores only those.
- Synthetic events use the `hidSystemState` source with explicit flags, so a physically held Shift does not leak in.
- The tap is re-armed on `TapDisabledByTimeout` / `TapDisabledByUserInput`, by a timer, after wake from sleep
  and when permissions change.
- Pasteboard: snapshot all types of all items, wait for `changeCount` instead of fixed sleeps, restore only if
  the user did not copy something new, mark our own writes as transient/concealed.

## Reliability Rules

- No `unwrap`/`expect` outside tests (`clippy::unwrap_used` and `clippy::expect_used` are denied).
- Every `extern "C"` callback body and the conversion pipeline run inside `catch_unwind`: a panic must neither
  cross the FFI boundary nor kill the app.
- CoreFoundation / AppKit resources are owned by RAII wrappers that free them in `Drop`.
- **Privacy**: the user's text lives in memory only. Never log its content (only its length), never persist it.

## Keyboard Layouts

- Layout IDs are macOS input source IDs (`"com.apple.keylayout.US"`).
- Character maps are derived from the system (`UCKeyTranslate`, with the keyboard type of the user's
  hardware) and put into `LayoutMaps`; built-in tables are the fallback and reference in tests.
- Adding a built-in layout: add the map string, register its IDs, add tests. The tests enforce 94 entries and
  no duplicate values.

## Localization Guidelines

1. **All user-facing strings** go through the localization module of the core - never hardcode UI text.
2. **Keys use namespace prefixes**: `menu.*`, `settings.*`, `general.*`, `smartMode.*`, `layoutSwitchMode.*`,
   `layouts.*`, `hotkey.*`, `hotkeyRecorder.*`, `permissions.*`, `about.*`, `alert.*`, `common.*`.
3. **When adding a key**: add it to **every** language file; a test fails if the key sets differ.
4. **Typography**: plain hyphens (" - ") instead of dashes, straight quotes, "=>" instead of arrows
   (enforced by test).
5. **Placeholders**: `%@` for text, `%d` for numbers.
6. **Fallback**: a missing key falls back to English, then to the raw key.
