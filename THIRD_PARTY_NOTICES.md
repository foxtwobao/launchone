# Third-party source provenance

LaunchOne is a minimal derivative of CC Switch's desktop configuration approach,
using Tauri 2, Rust, React and TypeScript. It does not embed CC Switch's full
provider database, router, UI or command surface.

- Upstream: https://github.com/farion1231/cc-switch
- Tag: `v4.0.26`
- Commit: `f621d4216189ef06bcc6acd3588c84937b9794e1`
- License: MIT, Copyright (c) 2025 Jason Young (see `LICENSE`)
- Copied source: `src-tauri/src/vendor/jsonc_document.rs`, from upstream
  `src-tauri/src/jsonc_document.rs`. This preserves JSONC/JSON5 comments and
  formatting while applying targeted configuration edits. Original tests retained.

All other LaunchOne-specific business code is implemented for this project.
Dependency licenses remain with their respective authors. Upstream application
identifiers, update endpoints and credentials are not included.
