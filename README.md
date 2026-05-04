# Supuraisu 🧁

Supuraisu is a compact macOS desktop client for legitimate Splice users. It focuses on fast browsing, library/likes/collections, reliable session persistence, downloading licensed samples, local playback, and native drag-out of downloaded audio files into DAWs.

<img width="805" height="604" alt="image" src="https://github.com/user-attachments/assets/968b5491-0dfc-4c2f-a391-8f20e096e8a1" />

## Scope

- Uses your own Splice account/session.
- Does **not** bypass subscriptions, licensing, payments, credits, or authorization.
- Requires Splice desktop resources installed locally for the helper-backed flow.
- Stores Supuraisu auth persistence in macOS Keychain with explicit user consent.

## Development

```bash
bun install
bun run tauri dev
```

Checks:

```bash
bun run check
cargo check --manifest-path src-tauri/Cargo.toml
```

## Packaging

```bash
bun run package:mac
```

See [`docs/RELEASE.md`](docs/RELEASE.md) for versioning, signing, notarization, GitHub Releases, and updater setup.

## Notes

Supuraisu currently targets macOS first. Native file drag-out is implemented with AppKit so downloaded files can be dragged into DAWs as real file drags.
