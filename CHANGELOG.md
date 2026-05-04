# Changelog

All notable Supuraisu changes should be documented here.

## 0.1.4

- Fix filter dropdown rendering and close behavior.
- Close filter dropdowns when clicking outside or pressing Escape.

## 0.1.3

- Add experimental scrambled preview decoding using the user's installed Splice.app WASM at runtime.
- Show decoder/WASM integration status in About diagnostics.
- Fix compact filter dropdown behavior.
- Improve filename searches by ignoring audio extensions.
- Auto-download purchased library samples as a fallback when scrambled preview decoding is unavailable.

## 0.1.2

- Configure macOS ad-hoc app signing for more consistent test bundles.

## 0.1.1

- Enable signed Tauri updater configuration and release metadata.

## 0.1.0

Initial macOS-focused development release.

- Supuraisu-owned Splice/Auth0 login with Keychain-backed refresh persistence.
- Supuraisu-hosted Splice Helper lifecycle without visible Splice window.
- Search, Library, Packs, Likes, Collections.
- Download flow for licensed/credit-confirmed samples.
- Local playback and native drag-out to DAWs.
- Real waveform JSON rendering with synthetic fallback.
