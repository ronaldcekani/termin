# QA Checklist: macOS Platform Layer (`termin-platform`)

This checklist verifies the native macOS AppKit integration implemented in Track 1C.
Test using the standalone validation example:

```bash
cargo run --example event_log
```

---

## 1. Window Management & Styling

- [ ] **Native Traffic Lights**:
  - Traffic lights (close, minimize, zoom) are visible in the top-left corner at standard macOS offsets.
  - Hovering over buttons displays close/minimize/zoom glyphs.
  - Clicking red button sends `PlatformEvent::CloseRequested` (does not abruptly destroy without app consent).
  - Clicking yellow button minimizes window into macOS Dock with standard genie/scale animation.
  - Clicking green button toggles native macOS full screen space.
- [ ] **Window Dragging Regions**:
  - Clicking and dragging within the top 38 pt title/tab region moves the window smoothly.
  - Clicking and dragging outside the top 38 pt (content area) delivers `PlatformEvent::Mouse` events to the app and does not drag the window.
- [ ] **Double-Click Action**:
  - Double-clicking the top 38 pt drag region respects the user's macOS system setting (`defaults read -g AppleActionOnDoubleClick`):
    - If set to `Maximize` (Zoom), the window expands to fill the usable screen area.
    - If set to `Minimize`, the window minimizes to the Dock.
- [ ] **Full Screen Transition (`Cmd+Ctrl+F`)**:
  - Pressing `Cmd+Ctrl+F` or 'f' key toggles native macOS full screen.
  - Transition smoothly animates to a new macOS space.
  - In full screen, the menu bar auto-hides when moving the mouse away and slides down when moving to the top edge.
  - Pressing `Cmd+Ctrl+F` again returns to the previous window position and size.
- [ ] **Opacity & Blur**:
  - Windows created with `opacity < 1.0` render with transparent background.
  - When `blur: Some(true)` is set, `NSVisualEffectView` under-window background blur activates behind the Metal layer.

---

## 2. Metal Rendering & Display Link

- [ ] **Dual Window Rendering**:
  - Running `cargo run --example event_log` opens two distinct windows:
    - Window 1 clears to Slate Blue (`rgb(0.12, 0.16, 0.28)`).
    - Window 2 clears to Crimson (`rgb(0.35, 0.08, 0.15)`).
- [ ] **Live Resize**:
  - Dragging any window border or corner resizes the window smoothly.
  - The Metal layer redraws continuously without black frames, flickering, or stretching (`layerContentsRedrawPolicy = .duringViewResize`).
- [ ] **Display Link Lifecycle (Idle Detection)**:
  - When no frames are requested, the display link pauses after 2 idle vsyncs (`set_paused(true)`).
  - CPU usage drops to 0.0% in Activity Monitor when the window is idle.
  - Interacting with the window (typing or scrolling) unpauses the display link immediately.
- [ ] **ProMotion 120Hz**:
  - On 120Hz ProMotion displays (MacBook Pro 14"/16"), animations and updates sync up to 120 fps.
- [ ] **Multiple Displays with Mixed Scaling**:
  - Drag a window between a Retina display (2.0x scale) and a standard external display (1.0x scale).
  - `viewDidChangeBackingProperties` fires, updating `CAMetalLayer.contentsScale` and `drawableSize`.
  - Window content remains sharp and does not appear blurry or improperly scaled.

---

## 3. Keyboard & IME (Input Method Editor)

- [ ] **Japanese IME**:
  - Switch macOS input source to Hiragana.
  - Type `nihon` → marked text appears underlined with composition candidates (`PlatformEvent::ImeCompose`).
  - Candidate popup window aligns correctly at the cursor rect (tested via `firstRectForCharacterRange:actualRange:`).
  - Press `Space` to cycle kanji (日本), press `Enter` to commit (`PlatformEvent::ImeCommit("日本")`).
- [ ] **Chinese IME (Pinyin)**:
  - Switch input source to Pinyin - Simplified.
  - Type `nihao` → candidate bar displays 你好. Press `1` or `Space` to commit.
- [ ] **Dead Keys & Accent Sequences**:
  - Using ABC keyboard layout:
    - Press `Option+e` followed by `e` → produces `é`.
    - Press `Option+u` followed by `u` → produces `ü`.
    - Press `Option+i` followed by `o` → produces `ô`.
    - Press `Option+` `~` followed by `a` → produces `ã`.
- [ ] **Option as Alt**:
  - With `option_as_alt = Left`:
    - Pressing Left Option + `b` sends `KeyEvent` with `Key::Char('b')`, `mods: ALT`.
    - Pressing Right Option + `e` then `e` produces `é` (diacritic preserved).
  - With `option_as_alt = Both`:
    - Both Option keys behave as Alt keys for shell word motions (`Alt+f`, `Alt+b`).
- [ ] **Emoji & Symbol Palette**:
  - Press `Ctrl+Cmd+Space`.
  - The macOS Character Palette appears.
  - Double-click an emoji (e.g. 🚀) → it is inserted into the event log via `ImeCommit` / `KeyEvent`.
- [ ] **Dictation**:
  - Press the Dictation shortcut (Fn twice or Microphone key).
  - Speak test words → marked text appears and commits as spoken words finish.

---

## 4. Mouse, Scrolling, & Cursors

- [ ] **Mouse Clicks and Movements**:
  - Left click, right click, and middle click deliver `PlatformEvent::Mouse` with `clickCount: 1`, `2` (double-click), or `3` (triple-click).
- [ ] **Smooth Trackpad Scrolling**:
  - Scrolling on a trackpad reports `has_precise_scrolling_deltas: true`.
  - Phases transition properly: `MayBegin` → `Began` → `Changed` → `Ended` → `MomentumPhase`.
- [ ] **Discrete Mouse Wheel**:
  - Using a physical mouse wheel reports discrete line scrolls without momentum phase.
- [ ] **Cursor Icons**:
  - Mouse hover over text regions changes the cursor to `IBeam`.
  - Mouse hover over drag dividers changes the cursor to `ResizeLeftRight` / `ResizeUpDown`.

---

## 5. File Drag & Drop

- [ ] **Finder File Drop**:
  - Select one or more files/folders in Finder and drag them over the Termin window.
  - The mouse cursor displays the green (+) badge indicating `Copy` operation is valid.
  - Release the files over the window.
  - Verify that `PlatformEvent::FileDrop(paths)` is logged with canonical filesystem paths.

---

## 6. Clipboard & Notifications

- [ ] **Clipboard Copy & Paste**:
  - Press 'c' in `event_log` example → copies `"Termin copied text from window..."` to `NSPasteboard`.
  - Paste into TextEdit or another app with `Cmd+V` → verified content matches.
  - Copy text in external app, then press 'v' in `event_log` → prints external clipboard text to stdout.
- [ ] **System Notifications**:
  - Press 'n' in `event_log` example → `UNUserNotificationCenter` posts a notification.
  - Unbundled binary (`cargo run`) logs notification and bounces the Dock icon (`requestUserAttention`).
  - Bundled `.app` displays native macOS banner with title and body.
  - Clicking banner focuses the window.
- [ ] **Dock Badge**:
  - Press 'b' in `event_log` example → Dock icon displays red badge number `1`.
  - Closing windows or clearing badge restores normal Dock icon.

---

## 7. Accessibility (VoiceOver)

- [ ] **VoiceOver Navigation**:
  - Enable VoiceOver (`Cmd+F5`).
  - Focus the Termin window.
  - Verify VoiceOver identifies the window as a text area and reads visible text.
  - Rapid output text changes are throttled to 2 notifications per second to prevent speech synthesizer buffer flooding.
