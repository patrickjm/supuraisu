# Release checklist

Supuraisu release engineering targets macOS first.

## Versioning

Use one command to keep the three version files aligned:

```bash
bun run version:set 0.1.1
```

This updates:

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Before tagging:

```bash
bun run release:check
```

## Local macOS package

```bash
bun run package:mac
```

Artifacts are emitted under `src-tauri/target/release/bundle/`.

## Updater signing key

Tauri updater artifacts must be signed with a minisign-compatible keypair. Generate it locally and store only the private key in GitHub Secrets:

```bash
bunx tauri signer generate --write-keys ~/.tauri/supuraisu-updater.key
```

Then set these repository secrets:

- `TAURI_SIGNING_PRIVATE_KEY` — contents of the generated private key
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — password used during generation, if any

The public key from generation should be placed in the updater config before enabling production update checks.

## Apple signing / notarization secrets

For Developer ID distribution, configure these GitHub Secrets:

- `APPLE_CERTIFICATE` — base64-encoded `.p12` Developer ID Application certificate
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY` — e.g. `Developer ID Application: Your Name (TEAMID)`
- `APPLE_ID`
- `APPLE_PASSWORD` — app-specific password
- `APPLE_TEAM_ID`

Without these, CI can still build unsigned/ad-hoc artifacts for testing, but Keychain prompts and Gatekeeper UX will be worse.

## GitHub release flow

1. `bun run version:set x.y.z`
2. `bun run release:check`
3. Commit changes.
4. Tag: `git tag vx.y.z && git push --tags`
5. GitHub Actions builds macOS artifacts and attaches them to the release.

## Updater endpoint

The intended updater endpoint shape is:

```text
https://github.com/patrickmoriarty/supuraisu/releases/latest/download/latest.json
```

Enable the updater endpoint/pubkey in `src-tauri/tauri.conf.json` once the updater keypair exists and the first signed release has been published.
