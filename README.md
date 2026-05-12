# MapWeave Lab

Rust + WebGPU map rendering practice project.

## Goals

- Build a self-owned rendering pipeline without third-party map SDKs.
- Run on native and web targets from a shared core.
- Start directly with MVT (Mapbox Vector Tile) ingestion.

## Workspace Layout

- `crates/mw-core`: shared map domain model and orchestration traits.
- `crates/mw-provider-mvt`: MVT tile provider abstraction and bootstrap.
- `crates/mw-render-wgpu`: WGPU renderer scaffolding and backend traits.
- `apps/native-viewer`: native executable entry.
- `apps/web-viewer`: web/wasm entry library.

## Quick Start

```bash
cargo check
cargo run -p native-viewer
```
