# MapWeave Lab v0 Architecture

## Scope

- Direct MVT-first pipeline.
- Shared Rust core for native and web.
- No third-party map rendering SDK.

## Workspace Structure

```text
MapWeave-Lab/
├── Cargo.toml                         # workspace root
├── apps
│   ├── native-viewer
│   │   └── src/main.rs                # native entry point
│   └── web-viewer
│       └── src/lib.rs                 # wasm entry point
├── crates
│   ├── mw-core
│   │   └── src
│   │       ├── lib.rs                 # module exports
│   │       ├── tile/mod.rs            # tile identity model
│   │       ├── feature/mod.rs         # normalized feature models
│   │       ├── scene/mod.rs           # tile scene + generic layer payload
│   │       └── error.rs               # core errors
│   ├── mw-provider-mvt
│   │   └── src
│   │       ├── lib.rs                 # module exports
│   │       ├── config.rs              # provider config
│   │       ├── provider/mod.rs        # orchestrates fetch -> decode -> map
│   │       ├── fetch/mod.rs           # HTTP fetching and cache-key strategy
│   │       ├── decode/mod.rs          # MVT decoding path
│   │       └── map/mod.rs             # source-layer -> scene-layer mapping
│   ├── mw-render-wgpu
│   │   └── src
│   │       ├── lib.rs                 # module exports
│   │       ├── renderer.rs            # renderer orchestration
│   │       └── layer
│   │           ├── mod.rs             # layer module exports
│   │           ├── trait.rs           # render-layer extension trait
│   │           └── background.rs      # background-layer placeholder
│   ├── mw-style
│   │   └── src
│   │       ├── lib.rs                 # module exports
│   │       ├── schema.rs              # paint/layout schema definitions
│   │       ├── resolver.rs            # feature -> draw params resolver
│   │       └── defaults.rs            # default style presets
│   └── mw-telemetry
│       └── src
│           ├── lib.rs                 # platform-select exports
│           ├── config.rs              # log config + levels
│           ├── native.rs              # env_logger init
│           └── web.rs                 # wasm_logger init
└── docs
    └── architecture-v0.md
```

## Layer Responsibilities

1. `mw-provider-mvt`: tile acquisition and decode (data only).
2. `mw-core`: normalized tile scene model and shared domain types.
3. `mw-style`: vector feature paint/layout config and draw param resolving.
4. `mw-render-wgpu`: renderer orchestration and GPU execution only.
5. `mw-telemetry`: cross-platform logging initialization.
6. `native-viewer` / `web-viewer`: platform entry points and composition root.

## Recommended Architecture Shape

Use a clearer three-stage pipeline:

1. **Data stage** (`mw-provider-mvt` + `mw-core`): fetch/decode and normalize features.
2. **Style stage** (`mw-style`): apply style rules and generate render-ready draw params.
3. **Render stage** (`mw-render-wgpu`): consume draw params and execute GPU draw calls.

This split avoids two common issues:

- renderer owning business/style logic;
- provider encoding render assumptions too early.

## Data Flow (Current v0 Skeleton)

1. App (`native-viewer`/`web-viewer`) boots and initializes logging via `mw-telemetry`.
2. App requests tiles from `mw-provider-mvt` through `TileProvider`.
3. Provider returns `mw-core::TileSceneData` with one or more `TileLayerData`.
4. App passes scene data into `mw-style` and resolves feature-level draw params.
5. App uploads style-resolved render payload into `mw-render-wgpu::Renderer`.
6. Renderer dispatches payload to registered render layers and executes draw.

## Provider vs Map (Critical Boundary)

This is the boundary that most easily causes confusion.

- `provider` is the **pipeline orchestrator**.
- `map` is the **data translator**.

### Responsibilities

- `provider`:
  - Owns config, URL template, retry policy, cache policy.
  - Calls `fetch`, then `decode`, then `map`.
  - Exposes one stable output to outside: `mw_core::TileSceneData`.
- `map`:
  - Receives decoded data only.
  - Applies source-layer -> domain-layer mapping rules.
  - Builds `TileLayerData` and `TileSceneData`.
  - Does not perform HTTP, caching, retries, or token handling.

### Call Chain (Expected)

1. `provider.fetch_tile(tile_id)`
2. `fetch.build_tile_url(...)` + `fetch.fetch_tile_bytes(...)`
3. `decode.decode_mvt_tile(...)`
4. `map.map_decoded_tile_to_scene(...)`
5. return `TileSceneData`

### Anti-Patterns (Do Not Do)

- Put mapping logic inside renderer.
- Put HTTP/token logic inside `map`.
- Let `mw-core` depend on MVT-specific types.
- Return decoded raw MVT structs to app/render layer.

## Next Implementation Steps

1. Add MVT decode path and map source layers into `TileSceneData`.
2. Define `mw-style` schema for line/fill/circle/text paint/layout fields.
3. Implement style resolver (feature properties + zoom -> draw params).
4. Add camera + visible tile scheduler in shared core logic.
5. Build line mesh generation and GPU buffer lifecycle from resolved params.
6. Wire `native-viewer` with `winit` render loop and surface setup.
7. Wire `web-viewer` wasm startup + canvas binding path.
