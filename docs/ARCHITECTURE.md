# Supuraisu architecture

Supuraisu is a Tauri desktop client for legitimate Splice users. It is not intended to bypass authentication, licensing, subscriptions, or payment checks.

## Phase 1 strategy

Reuse the installed Splice native helper and replace the desktop UI/session handling.

```text
Supuraisu Tauri UI
  -> Rust backend
  -> local gRPC client generated from app.proto
  -> Splice desktop-helper on 127.0.0.1:56765-56785
  -> official Splice cloud/download behavior
```

## Installed Splice artifacts used during development

- App: `/Applications/Splice.app`
- Helper: `/Applications/Splice.app/Contents/Resources/desktop-helper`
- Local TLS cert: `~/Library/Application Support/com.splice.Splice/.certs/cert.pem`
- Local TLS key: `~/Library/Application Support/com.splice.Splice/.certs/key.pem`
- Helper gRPC port range: `56765-56785`

## Major subsystems

### Helper gRPC client

Generate Rust bindings from a compatible `proto.App` schema. Initial calls:

- `UserPreferences`
- `GetSession`
- `SearchSamples`
- `SampleInfo`
- `DownloadSample`
- `CancelSampleDownload`

### Auth/session manager

Implement normal Auth0 login once, then robust refresh-token persistence.

- Auth domain: `auth.splice.com`
- Audience: `https://splice.com`
- Scope: `openid profile offline_access`
- Refresh tokens stored in OS keychain
- Correctly persist rotated refresh tokens before continuing

### Native drag export

Do not rely on WebView HTML5 drag for DAWs. Implement native file drag source.

- macOS: AppKit pasteboard file URLs
- Windows: COM `CF_HDROP`
- Linux: GTK/GDK `text/uri-list`

A file must exist locally before drag starts. If a sample is not downloaded, prepare/download first, then allow drag.

## First spikes

1. Verify Tauri shell builds.
2. Add installed Splice environment check. ✅
3. Generate Rust gRPC client from `app.proto`.
4. Connect to helper via local TLS.
5. Call read-only helper methods.
6. Implement auth persistence.
7. Spike macOS native file drag-out.
