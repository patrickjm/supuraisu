# Supuraisu 🧁

Supuraisu is an alternative macOS desktop client for Splice. It focuses on fast browsing, library/likes/collections, reliable session persistence, downloading licensed samples, local playback, and native drag-out of downloaded audio files into DAWs.

<img width="867" height="664" alt="Greenshot 2026-05-04 11 15 39" src="https://github.com/user-attachments/assets/1b5bdbe5-556d-4cc6-8f98-45067681dce4" />

## Scope

- Uses your own Splice account/session.
- Requires Splice desktop resources installed locally for the helper-backed flow.
- Stores Supuraisu auth persistence in macOS Keychain.

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
