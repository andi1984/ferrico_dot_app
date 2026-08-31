# Android release signing

Release APKs attached to GitHub releases are signed with a single, long-lived
"upload" keystore. Android identifies an app by `applicationId` **plus** signing
certificate: if the certificate changes between versions, the device refuses to
install the update over the existing app (`INSTALL_FAILED_UPDATE_INCOMPATIBLE`).

That is exactly what the old setup did wrong — releases were built with
`--debug`, and the debug keystore is auto-generated per machine, so every CI
runner (and every release) had a different signature. Closes the loop on #74.

## How it works

- `src-tauri/gen/android/app/build.gradle.kts` defines a `release` signing
  config that reads `keystore.properties` from the same directory. Both
  `keystore.properties` and `*.keystore` are gitignored — the keystore and its
  passwords must never be committed.
- If `keystore.properties` is absent, a `--release` build still compiles but
  produces an **unsigned** (uninstallable) APK. Debug builds are unaffected.
- The `android` job in `.github/workflows/release.yml` recreates the keystore
  and `keystore.properties` from repo secrets, then runs
  `tauri android build --apk --split-per-abi --target aarch64 armv7`.

## One-time setup

1. Generate the keystore (keep it somewhere safe *outside* the repo — losing it
   means users have to uninstall/reinstall once more when the key changes):

   ```bash
   keytool -genkeypair -v \
     -keystore ferrico-upload.keystore \
     -alias upload \
     -keyalg RSA -keysize 2048 \
     -validity 10000
   ```

2. Add the four repo secrets (Settings → Secrets and variables → Actions, or
   `gh secret set`):

   ```bash
   base64 -w0 ferrico-upload.keystore | gh secret set ANDROID_KEYSTORE_B64
   gh secret set ANDROID_KEYSTORE_PASSWORD   # store password from step 1
   gh secret set ANDROID_KEY_ALIAS --body upload
   gh secret set ANDROID_KEY_PASSWORD        # key password (often = store password)
   ```

The release job fails loudly if any secret is missing — an unsigned release APK
would only produce confusing "app not installed" errors for users.

## Local release builds

Drop the keystore next to the gradle file and describe it in
`src-tauri/gen/android/app/keystore.properties`:

```properties
storeFile=ferrico-upload.keystore
storePassword=...
keyAlias=upload
keyPassword=...
```

Then `bun run android:build -- --apk --split-per-abi --target aarch64` builds an
installable arm64 release APK.

## Migration note (one-time breakage)

Devices that installed one of the old debug-signed release APKs cannot upgrade
to the properly signed builds either — the signature changed one last time.
Uninstall Ferrico once (Neon-synced data restores itself; local-only data needs
an export first) and install the new APK. From then on, updates install over
each other normally.

## Why the APK got ~30x smaller

The old asset was a `--debug` **universal** APK: unstripped debug `.so` files
(the gradle debug build type keeps debug symbols for all ABIs) × 4 architectures
in one file — nearly 1 GB. The release build strips symbols and minifies, and
`--split-per-abi` ships one APK per architecture instead of bundling all of
them. x86/x86_64 targets are omitted entirely — they only exist for emulators.
Pick `arm64-v8a` on any phone from ~2016 onwards; `armeabi-v7a` is the fallback
for very old 32-bit devices.
