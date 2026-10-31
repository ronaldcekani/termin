# Spec: macOS Platform Layer (`termin-platform`)

All AppKit code. Implements the `Platform` / `AppHandler` / `PlatformCx` contract from `architecture.md §3.4`. Uses `objc2` + `objc2-app-kit` + `objc2-foundation` + `objc2-quartz-core` + `objc2-user-notifications` with **only the needed features enabled**.

## 1. Application lifecycle

- `NSApplication.sharedApplication`, activation policy `.regular`.
- Custom `AppDelegate` (declared with `objc2::define_class!`) forwarding:
  - `applicationDidFinishLaunching` → `AppHandler::init`.
  - `applicationShouldTerminate` → `AppHandler::should_terminate` (close confirmation when processes running).
  - `applicationShouldHandleReopen` → open a window if none.
  - `application:openURLs:` / `openFiles` → open a new tab at the folder (dragging a folder onto the Dock icon).
- No nib/storyboard. Main menu built in code from a `MenuSpec` produced by the app (so keybinding labels in menus reflect config).
- `NSApp.run()`; `Platform::run` never returns.

## 2. Windows

- `NSWindow` with style mask `titled | closable | miniaturizable | resizable | fullSizeContentView`, `titlebarAppearsTransparent = true`, `titleVisibility = hidden`, toolbar-less. Traffic lights stay in standard position; the app reserves a 38 pt drag region at the top of the sidebar and tab strip.
- Window dragging: `NSView.mouseDownCanMoveWindow` returns true only for drag regions reported by the UI hit-test (`HitKind::WindowDrag`); implemented by calling `window.performWindowDragWithEvent` on mouse-down in those regions.
- Double-click on drag region → respect `AppleActionOnDoubleClick` user default (zoom/minimize).
- Native full screen supported; tabbing mode `NSWindowTabbingModeDisallowed` (we have our own tabs).
- `WindowOpts { frame: Option<Rect>, restore_id: Option<u64> }`. Frame autosave handled by `termin-persist`, not `setFrameAutosaveName`.
- Background: when `window.opacity < 1`, `isOpaque = false`, clear color with alpha, optional blur via `NSVisualEffectView` behind the Metal view (`window.blur = true`, material `.underWindowBackground`).

## 3. TerminView

Single `NSView` subclass per window (content view) hosting the `CAMetalLayer` (`wantsLayer = true`, `makeBackingLayer` returns `CAMetalLayer`, `layerContentsRedrawPolicy = .duringViewResize`). The whole window UI (sidebar, tabs, panes, overlays) is drawn by us into this one layer — no nested NSViews, which keeps memory and layout overhead minimal.

Overrides:

| Selector | Behavior |
|---|---|
| `acceptsFirstResponder` | `YES` |
| `keyDown:` | if marked text or not handled by keybinding pre-check → `interpretKeyEvents:`; see §4 |
| `keyUp:` | forward release (kitty protocol event types) |
| `flagsChanged:` | modifier tracking; modifier-only keybindings |
| `performKeyEquivalent:` | let menu handle Cmd shortcuts first; return `NO` for ones the app wants as raw keys (configurable `macos.cmd_passthrough`) |
| `mouseDown/Up/Dragged/Moved`, `right*`, `other*` | `PlatformEvent::Mouse` with `clickCount` |
| `scrollWheel:` | `PlatformEvent::Scroll` with `hasPreciseScrollingDeltas`, phase, momentumPhase |
| `updateTrackingAreas` | single tracking area covering the view (`mouseMoved`, `activeAlways`, `inVisibleRect`) |
| `viewDidChangeBackingProperties` | scale change → `Resized` |
| `setFrameSize:` | `Resized` + synchronous render during live resize |
| `viewDidChangeEffectiveAppearance` | `Appearance(Light/Dark)` |
| `draggingEntered/performDragOperation` | file URLs → `FileDrop` (app inserts shell-escaped paths) |
| `menuForEvent:` | right-click context menu (native `NSMenu`, built from app-provided `MenuSpec`) |

Cursor icons via `NSCursor` (`arrow`, `IBeam`, `pointingHand`, `resizeLeftRight`, `resizeUpDown`), set on mouse move from hit-test result.

## 4. Keyboard and IME

Implement `NSTextInputClient` on `TerminView`:

- `insertText:replacementRange:` → `ImeCommit` (or `Key` with text when it is a plain keystroke result — track via a `in_key_down` flag so a keystroke producing text is delivered once as `KeyEvent{text: Some}`).
- `setMarkedText:selectedRange:replacementRange:` → `ImeCompose`.
- `unmarkText`, `hasMarkedText`, `markedRange`, `selectedRange`, `validAttributesForMarkedText` (empty array).
- `firstRectForCharacterRange:actualRange:` → screen rect from the last `set_ime_rect` (cursor cell or editor caret).
- `doCommandBySelector:` → map selectors (`insertNewline:`, `deleteBackward:`, `moveLeft:`, …) to `Key` events when not consumed by IME so dead keys/compose work but special keys still reach the app.

Key translation:
- `Key` (logical) from `charactersIgnoringModifiers` + `NSEvent.keyCode` table for named keys.
- `option_as_alt` (`left | right | both | none`) applied before `interpretKeyEvents`: when Option should act as Alt, bypass IME and emit `Key` with `ALT` modifier and the base character.
- Physical keycode passed through for kitty protocol and position-based bindings.

## 5. Display link

- macOS 13: `CVDisplayLinkCreateWithActiveCGDisplays` + output callback on the CV thread → post to main via `dispatch_async_f` on the main queue (coalesced by atomic flag) → `AppHandler::frame`.
- macOS 14+: `NSView.displayLink(target:selector:)` (`CADisplayLink`) — runs on main run loop, auto-tracks the window's screen. Runtime check with `NSProcessInfo.isOperatingSystemAtLeastVersion`.
- Display link per window, paused when no frame requested (see rendering §3.5).
- Screen change (window moved to another display) → update display link + refresh rate.

## 6. Wake from IO thread

`WakeHandle` wraps a `CFRunLoopSourceRef` (version 0) added to the main run loop in common modes. `wake()` = `CFRunLoopSourceSignal` + `CFRunLoopWakeUp`. Callback → `AppHandler::wake`. Cheap, coalesces naturally, works during menu tracking and live resize (common modes).

## 7. Clipboard

`NSPasteboard.generalPasteboard`: get/set `NSPasteboardTypeString`. File URLs on paste → shell-escaped paths. Paste of text into terminal: apply bracketed paste if mode set; warn on multi-line paste into a classic-mode shell (configurable `terminal.warn_multiline_paste`).

## 8. System notifications

- `UNUserNotificationCenter.currentNotificationCenter`, request `.alert | .sound | .badge` authorization lazily on first notification.
- Only posted when the app is not active or the source pane is not visible (see app spec §Attention).
- `userInfo` carries `{window, workspace, pane, tab}` ids; delegate `didReceiveNotificationResponse` → focus that tab.
- Dock badge: `NSApp.dockTile.badgeLabel` = count of tabs with unread attention (empty when 0).
- Requires running as a bundled `.app` (identifier `dev.termin.Termin`); when run unbundled via `cargo run`, notifications degrade to dock bounce (`requestUserAttention`) and a log line.

## 9. Appearance

- Report system appearance; theme config may be `theme = "auto"` → pick `theme.light` / `theme.dark`.
- `NSWindow.appearance` set to match active theme (affects traffic lights and native menus).

## 10. Accessibility (minimum v1)

- `TerminView` exposes `NSAccessibilityRole.textArea` for the focused pane with `accessibilityValue` = visible text, `accessibilitySelectedTextRange`, and posts `valueChanged` notifications throttled to 2/s. Enough for VoiceOver to read output. Full element tree for sidebar/tabs is phase 4.

## 11. Main menu (generated)

```
Termin:   About, Settings… (opens config.toml in default editor), Reload Config, Quit
Shell:    New Window ⌘⇧N, New Workspace ⌘N, New Tab ⌘T, Split Right ⌘D, Split Down ⌘⇧D,
          Close Tab ⌘W, Close Pane ⌘⌥W, Close Workspace ⌘⇧W, Rename Tab…, Rename Workspace…
Edit:     Copy ⌘C, Paste ⌘V, Select All ⌘A, Find ⌘F, Find Next ⌘G, Find Previous ⌘⇧G, Clear Buffer ⌘K
View:     Command Palette ⌘⇧P, Go to… ⌘P, Toggle Sidebar ⌘B, Zoom Pane ⌘⇧↩, Font Bigger ⌘+, Smaller ⌘-, Reset ⌘0,
          Toggle Block Mode, Enter Full Screen
Window:   Minimize, Zoom, Next/Prev Workspace ⌘⌥↓/↑ (also ⌃⌘]/[ ), Select Workspace 1–9 ⌘1–9,
          Next/Prev Tab ⌘⇧]/[, Focus Pane ⌘⌥←→↑↓, Jump to Latest Attention ⌘⇧U, window list
Help:     Termin Docs (opens local README in browser)
```

Shortcut strings come from the active keymap (config can rebind; menu reflects it).

## 12. Testing

- Most logic sits in the app crate; platform is thin. Tests: key translation table unit tests (keycode → `Key`), option-as-alt behavior, a manual QA checklist in `docs/qa/platform.md` (created in Phase 1) covering IME (Japanese, Pinyin, dead keys `´ e`), emoji picker (`⌃⌘Space`), dictation, full screen, multiple displays with different scale factors, ProMotion.
