# Blocksmith

Blocksmith is a Windows-first Minecraft Java launcher built with Tauri, React, TypeScript, and Rust. It focuses on isolated profiles, direct launch workflows, Modrinth content management, Microsoft account support, and local profile portability.

## Features

- Create isolated Vanilla and Fabric profiles
- Resolve Mojang versions, Fabric loaders, and launch plans
- Download managed Mojang Java runtimes when required
- Search Modrinth, review installs, and manage updates
- Import Modrinth `.mrpack` files
- Sign in with Microsoft and store refresh tokens in Windows Credential Manager
- Import and apply skins for supported accounts
- Export and import shareable profile manifests

## Requirements

- Windows 10 or Windows 11
- Node.js and `npm`
- Rust stable toolchain with Cargo
- Microsoft Visual Studio C++ build tools for Tauri/Rust Windows builds
- Microsoft Edge WebView2 Runtime

## Quick Start

```powershell
npm install
npm run tauri dev
```

For convenience on Windows, you can also double-click [`Run-Blocksmith.bat`](./Run-Blocksmith.bat).

## Build And Verify

```powershell
npm run build
cargo check --manifest-path .\src-tauri\Cargo.toml
cargo test --manifest-path .\src-tauri\Cargo.toml
```

## Configuration

Blocksmith does not require a `.env` file for local development.

- Set `microsoft_client_id` inside the app's Settings screen if you want live Microsoft/Minecraft sign-in.
- Leave `managed_runtime_preference` on `auto` unless you want to force a specific local Java install.
- App data is stored under `%LocalAppData%\Blocksmith`.

## Security Notes

- This repository does not ship with live OAuth credentials or refresh tokens.
- Microsoft refresh tokens are stored in Windows Credential Manager, not in the repository.
- Local launcher state, caches, logs, and profile data live outside the repo under `%LocalAppData%\Blocksmith`.

## Project Layout

```text
src/          React UI
src-tauri/    Tauri + Rust backend
```

## License

This project is licensed under the MIT License. See [`LICENSE`](./LICENSE).
