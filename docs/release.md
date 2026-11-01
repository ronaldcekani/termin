# Termin Release Engineering Guide

This guide describes the complete end-to-end release process for **Termin**, from version bumping and multi-architecture compilation to hardened runtime codesigning, Apple notarization, stapling, distribution packaging, and clean VM verification.

---

## 1. Overview & Architecture

Termin distributes native macOS application bundles (`Termin.app`), compressed distribution archives (`Termin.app.zip`), and drag-and-drop installer disk images (`Termin.dmg`).

### Build Targets & Deliverables
- **Target OS:** macOS 13.0+ (Ventura and later)
- **Architectures:**
  - `aarch64-apple-darwin` (Apple Silicon: M1/M2/M3/M4)
  - `x86_64-apple-darwin` (Intel 64-bit)
  - **Universal Mach-O:** Dual-architecture fat binary assembled via `lipo -create`
- **Resource & Size Budgets (`budgets.toml`):**
  - Zipped application bundle: **≤ 4.0 MiB** (stretch: ≤ 3.0 MiB)
  - Stripped binary (arm64): **≤ 8.0 MiB** (stretch: ≤ 5.0 MiB)
  - Embedded resources: **≤ 600 KiB**

All packaging automation is driven by `cargo xtask bundle`.

---

## 2. Prerequisites & Credentials

### Tools Required
- macOS 13.0+ host with Xcode 15+ command-line tools installed:
  ```bash
  xcode-select --install
  ```
- Rust stable toolchain:
  ```bash
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  ```
- Command-line utilities: `codesign`, `notarytool`, `stapler`, `ditto`, `hdiutil`, `lipo`, `spctl`.

### Apple Developer Accounts & Signing Identities
To distribute outside the Mac App Store, macOS Gatekeeper requires a valid **Developer ID Application** certificate and **Apple Notarization**.

1. **Signing Certificate:**
   Installed in macOS Keychain:
   ```bash
   security find-identity -v -p codesigning
   # Looks like: "Developer ID Application: Example Inc. (TEAMID1234)"
   ```
2. **Notarization Credentials:**
   Configure a stored keychain profile for `notarytool` (or supply API keys in CI):
   ```bash
   xcrun notarytool store-credentials "TerminNotary" \
     --apple-id "developer@example.com" \
     --team-id "TEAMID1234" \
     --password "<app-specific-password>"
   ```
   Or with an App Store Connect API Key:
   ```bash
   xcrun notarytool store-credentials "TerminNotary" \
     --key "/path/to/AuthKey_ABCD1234EF.p8" \
     --key-id "ABCD1234EF" \
     --issuer "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
   ```

---

## 3. Step-by-Step Release Procedure

### Step 1: Bump Version & Update Documentation
1. In the root `Cargo.toml`, update `[workspace.package].version`:
   ```toml
   [workspace.package]
   version = "1.0.0"
   ```
2. Run `cargo check --workspace` to update `Cargo.lock`.
3. Verify `resources/Info.plist` uses the `{{VERSION}}` placeholder for `CFBundleShortVersionString` and `CFBundleVersion`.
4. Commit the version bump:
   ```bash
   git commit -am "chore(release): bump version to 1.0.0"
   ```

---

### Step 2: Compile Binaries for Release

Build release binaries for both arm64 and x86_64 targets:
```bash
# 1. Apple Silicon (arm64)
cargo build --release --target aarch64-apple-darwin -p termin

# 2. Intel (x86_64)
cargo build --release --target x86_64-apple-darwin -p termin
```

Verify binary sizes vs budgets:
```bash
cargo xtask size
```

---

### Step 3: Assemble macOS Application Bundle (`cargo xtask bundle`)

Use `cargo xtask bundle` to assemble the bundle layout:

#### Assembling a Universal Bundle
```bash
cargo xtask bundle --universal --archive --dmg
```

#### Assembling Single Architecture (arm64)
```bash
cargo xtask bundle --target aarch64-apple-darwin --archive
```

The bundler automatically:
- Creates `target/release/bundle/Termin.app`
- Writes `Contents/Info.plist` with stamped version from root `Cargo.toml`
- Writes `Contents/PkgInfo` (`APPL????`)
- Combines architectures with `lipo` into `Contents/MacOS/termin` with `0755` permissions
- Copies `resources/Termin.icns`, `resources/shell/`, and `resources/themes/` into `Contents/Resources/`
- Creates compressed zip archive (`ditto -c -k --keepParent`)
- Verifies archive size against `budgets.toml`

---

### Step 4: Hardened Runtime Codesigning

macOS Gatekeeper requires Developer ID signing with the **Hardened Runtime** enabled:

```bash
SIGNING_IDENTITY="Developer ID Application: Example Inc. (TEAMID1234)"

# Sign the entire application bundle
codesign --force --deep --options runtime --timestamp \
  --sign "$SIGNING_IDENTITY" \
  target/release/bundle/Termin.app
```

#### Verify Signature
```bash
# 1. Verify bundle integrity
codesign --verify --deep --strict --verbose=2 target/release/bundle/Termin.app

# 2. Display certificate and runtime flags
codesign -dvvv target/release/bundle/Termin.app
# Must show: flags=0x10000(runtime) or flags=0x10002(adhoc,runtime)
```

---

### Step 5: Apple Notarization & Stapling

Apple notarization verifies that the application contains no malicious software.

1. **Submit for Notarization:**
   ```bash
   xcrun notarytool submit target/release/bundle/Termin.app.zip \
     --keychain-profile "TerminNotary" \
     --wait
   ```

2. **Check Submission Status:**
   If the status is `Accepted`, proceed to stapling. If `Invalid`, inspect the error log:
   ```bash
   xcrun notarytool log <SUBMISSION_ID> --keychain-profile "TerminNotary"
   ```

3. **Staple the Notarization Ticket:**
   Stapling embeds the ticket directly into the application bundle so Gatekeeper can validate it even offline:
   ```bash
   xcrun stapler staple target/release/bundle/Termin.app
   ```

   If a `.dmg` installer is also distributed, staple the DMG as well:
   ```bash
   xcrun stapler staple target/release/bundle/Termin.dmg
   ```

4. **Verify Stapling & Gatekeeper Assessment:**
   ```bash
   # Validate stapled ticket
   xcrun stapler validate target/release/bundle/Termin.app

   # Evaluate Gatekeeper compliance
   spctl --assess --type execute --verbose target/release/bundle/Termin.app
   # Must output: "target/release/bundle/Termin.app: accepted"
   ```

5. **Re-pack Final Stapled Zip:**
   Because stapling modifies files inside `Termin.app`, rebuild the final zip archive:
   ```bash
   ditto -c -k --keepParent \
     target/release/bundle/Termin.app \
     target/release/bundle/Termin-macos-universal.zip
   ```

---

## 4. CLI Symlink Installation (`termin` in PATH)

Termin provides a built-in palette command:
> **Command Palette:** `Install CLI in PATH` (`Action::InstallCli`)

### Installation Flow
1. **Source Discovery:** Resolves current running executable via `std::env::current_exe()` (e.g. `/Applications/Termin.app/Contents/MacOS/termin`).
2. **System Location (`/usr/local/bin/termin`):**
   - If `/usr/local/bin` is user-writable, installs or replaces the symlink atomically.
3. **User Fallback (`~/.local/bin/termin`):**
   - If system location is not writable, creates `~/.local/bin` if needed and installs the symlink there.
4. **Administrative Elevation (Optional):**
   - For users wanting `/usr/local/bin` when non-writable, prompts standard macOS authentication dialog via `osascript`:
     ```bash
     osascript -e 'do shell script "mkdir -p /usr/local/bin && ln -sf /Applications/Termin.app/Contents/MacOS/termin /usr/local/bin/termin" with administrator privileges'
     ```

### Verification
```bash
which termin
termin --version
termin --help
```

---

## 5. Clean macOS 13 VM Verification Checklist

Before publishing any release build to users, execute this checklist on a pristine, non-developer macOS 13.0 (Ventura) virtual machine (e.g. Tart, UTM, or Parallels):

- [ ] **Quarantine Simulation:**
  Simulate download by attaching the quarantine xattr:
  ```bash
  xattr -w com.apple.quarantine "0081;00000000;Safari;..." Termin.app.zip
  unzip Termin.app.zip
  ```
- [ ] **Gatekeeper First-Launch:**
  Double-click `Termin.app` in Finder.
  - Expected: macOS displays "Termin is an app downloaded from the Internet. Are you sure you want to open it?" with an "Open" button (NOT "cannot be opened because the developer cannot be verified").
  - Click "Open". Termin opens to the default terminal workspace within ≤ 120 ms.
- [ ] **Basic Terminal & Shell Functionality:**
  - Launch default shell (`/bin/zsh`).
  - Verify terminal output, ANSI colors, and cursor movement.
  - Test command execution (`ls -la`, `top`, `vim`, `git status`).
  - Test split pane (`Cmd+D` / `Cmd+Shift+D`) and tabs (`Cmd+T`).
- [ ] **Palette & CLI Install:**
  - Open palette (`Cmd+Shift+P` / `Cmd+P`), run `Install CLI in PATH`.
  - Open a new terminal tab and run `termin --version`.
- [ ] **Clean Shutdown:**
  - Quit Termin (`Cmd+Q`). Verify process exits cleanly with code 0 and no orphan pty processes.

---

## 6. GitHub Actions Automated Release Workflow

Tagged releases (`v*`) trigger `.github/workflows/release.yml`:
1. Checks out repository and sets up macOS runner (`macos-14`).
2. Enforces size budget verification (`cargo xtask size`).
3. Compiles `aarch64-apple-darwin` and `x86_64-apple-darwin` release targets.
4. Assembles universal `.app` bundle via `cargo xtask bundle --universal --archive --dmg`.
5. Imports Apple Developer ID certificate from GitHub Secrets (`APPLE_CERTIFICATE_BASE64`, `APPLE_CERTIFICATE_PASSWORD`).
6. Submits archives to Apple Notary Service (`APPLE_NOTARY_API_KEY`, `APPLE_NOTARY_KEY_ID`, `APPLE_NOTARY_ISSUER_ID`).
7. Staples notarization ticket to `.app` and `.dmg`.
8. Creates GitHub Release with:
   - `Termin-macos-arm64.zip`
   - `Termin-macos-universal.zip`
   - `Termin.dmg`
