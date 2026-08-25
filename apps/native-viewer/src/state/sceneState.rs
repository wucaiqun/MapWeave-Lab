use std::collections::{HashMap, HashSet};
use std::time::Instant;

use anyhow::Context;
use mw_async_data::{DataJob, DataResult, DataTaskRuntime};
use mw_core::{
    merge_tiles_into_scene_relative, tile_world_origin, world_units_per_tile, LayerPayload, TileId,
    TileSceneData,
};
use mw_provider_mvt::MvtProviderConfig;
use mw_render_wgpu::{BackgroundLayer, BuildingsLayer, Renderer, RendererConfig, RoadsLayer};
use mw_telemetry::{elapsed_ms, print_perf_if};

use super::CameraState;

/// How many new TileFetch jobs to submit per frame (fills up to runtime concurrency).
const MAX_TILE_SUBMITS_PER_FRAME: usize = 9;

/// Rebase camera-relative mesh when the look-at drifts farther than this (world units).
const MESH_REBASE_DISTANCE: f64 = 512.0;

fn format_tile_ids(tiles: &HashSet<TileId>, limit: usize) -> String {
    let mut ids: Vec<_> = tiles.iter().copied().collect();
    ids.sort_by_key(|t| (t.z, t.x, t.y));
    let total = ids.len();
    let preview: Vec<String> = ids
        .into_iter()
        .take(limit)
        .map(|t| format!("{}/{}/{}", t.z, t.x, t.y))
        .collect();
    if total > limit {
        format!("{}…(+{})", preview.join(","), total - limit)
    } else {
        preview.join(",")
    }
}

pub struct SyncTimings {
    pub total_ms: f64,
    pub tile_fetch_ms: f64,
    pub merge_ms: f64,
    pub upload_ms: f64,
    pub tiles_fetched: u32,
}

pub struct SceneState {
    pub renderer: Renderer,
    data_runtime: DataTaskRuntime,
    provider_config: MvtProviderConfig,
    provider_ready: bool,
    provider_init_inflight: bool,
    tiles: HashMap<TileId, TileSceneData>,
    visible: HashSet<TileId>,
    merged: TileSceneData,
    /// Camera origin used for the last GPU upload (camera-relative mesh).
    mesh_origin: Option<[f64; 2]>,
}

impl SceneState {
    /// Origin used for the currently uploaded GPU mesh (`world - origin`).
    pub fn mesh_origin(&self) -> Option<[f64; 2]> {
        self.mesh_origin
    }

    pub fn new() -> anyhow::Result<Self> {
        let mut renderer = Renderer::new(RendererConfig {
            clear_color: wgpu::Color {
                r: 0.02,
                g: 0.02,
                b: 0.08,
                a: 1.0,
            },
        });
        renderer.add_layer(Box::new(BackgroundLayer::default()));
        renderer.add_layer(Box::new(RoadsLayer::default()));
        renderer.add_layer(Box::new(BuildingsLayer::default()));

        let data_runtime = DataTaskRuntime::new().context("failed to create data task runtime")?;
        let provider_config = MvtProviderConfig::default();
        data_runtime.submit(DataJob::ProviderInit {
            config: provider_config.clone(),
        });

        Ok(Self {
            renderer,
            data_runtime,
            provider_config,
            provider_ready: false,
            provider_init_inflight: true,
            tiles: HashMap::new(),
            visible: HashSet::new(),
            merged: TileSceneData {
                tile_id: TileId::new(0, 0, 0),
                layers: vec![],
            },
            mesh_origin: None,
        })
    }

    pub fn prepare(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> anyhow::Result<()> {
        self.renderer.prepare(device, surface_format)
    }

    fn drain_data_results(&mut self, tile_fetch_ms: &mut f64, tiles_fetched: &mut u32) -> bool {
        let mut loaded_any = false;
        for result in self.data_runtime.drain() {
            match result {
                DataResult::ProviderReady { endpoint } => {
                    log::info!("tile endpoint: {endpoint}");
                    self.provider_ready = true;
                    self.provider_init_inflight = false;
                }
                DataResult::ProviderFailed { error } => {
                    log::warn!("tile endpoint not ready yet: {error}");
                    self.provider_ready = false;
                    self.provider_init_inflight = false;
                }
                DataResult::TileFetched {
                    tile_id,
                    scene,
                    elapsed_ms,
                } => {
                    *tile_fetch_ms += elapsed_ms;
                    *tiles_fetched += 1;
                    log::info!(
                        "loaded tile {}/{}/{} in {elapsed_ms:.1}ms ({} bg, {} bldg, {} roads)",
                        scene.tile_id.z,
                        scene.tile_id.x,
                        scene.tile_id.y,
                        count_background(&scene),
                        count_buildings(&scene),
                        count_roads(&scene),
                    );
                    self.tiles.insert(tile_id, scene);
                    loaded_any = true;
                }
                DataResult::TileFailed { tile_id, error } => {
                    log::warn!(
                        "failed to fetch tile {}/{}/{}: {error}",
                        tile_id.z,
                        tile_id.x,
                        tile_id.y,
                    );
                }
            }
        }
        loaded_any
    }

    fn ensure_provider_requested(&mut self) {
        if self.provider_ready || self.provider_init_inflight {
            return;
        }
        self.provider_init_inflight = true;
        self.data_runtime.submit(DataJob::ProviderInit {
            config: self.provider_config.clone(),
        });
    }

    /// Non-blocking tile sync: submit jobs, drain results, merge/upload on main thread.
    pub fn sync_visible_tiles(
        &mut self,
        camera: &mut CameraState,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> SyncTimings {
        let sync_start = Instant::now();
        let mut tile_fetch_ms = 0.0;
        let mut merge_ms = 0.0;
        let mut upload_ms = 0.0;
        let mut tiles_fetched = 0u32;

        camera.refresh_tile_zoom();
        let tile_zoom = camera.zoom();

        // Drop other-zoom cache entries so sticky never mixes LOD layers.
        let pruned_other_zoom = self.tiles.keys().any(|id| id.z != tile_zoom);
        if pruned_other_zoom {
            self.tiles.retain(|id, _| id.z == tile_zoom);
            self.visible.retain(|id| id.z == tile_zoom);
        }

        let loaded_any = self.drain_data_results(&mut tile_fetch_ms, &mut tiles_fetched);
        self.ensure_provider_requested();

        if !self.provider_ready {
            return SyncTimings {
                total_ms: elapsed_ms(sync_start),
                tile_fetch_ms,
                merge_ms,
                upload_ms,
                tiles_fetched,
            };
        }

        let mut wanted: Vec<TileId> = camera.visible_tiles();
        let origin = camera.target_world();
        wanted.sort_by_key(|tile| {
            let [ox, oz] = tile_world_origin(*tile);
            let half = world_units_per_tile(tile.z) * 0.5;
            let cx = ox + half;
            let cz = oz + half;
            let dx = cx - origin[0];
            let dz = cz - origin[1];
            ((dx * dx + dz * dz) * 1_000.0) as u64
        });
        let wanted_set: HashSet<TileId> = wanted.iter().copied().collect();

        let mut submitted = 0usize;
        for (priority, tile_id) in wanted.iter().copied().enumerate() {
            if submitted >= MAX_TILE_SUBMITS_PER_FRAME {
                break;
            }
            if self.tiles.contains_key(&tile_id) {
                continue;
            }
            if self.data_runtime.is_tile_in_flight(tile_id) {
                continue;
            }
            self.data_runtime.submit(DataJob::TileFetch {
                tile_id,
                priority: priority as u64,
            });
            submitted += 1;
        }

        // Drain again in case fast cache hits completed during submit.
        let loaded_any = loaded_any || self.drain_data_results(&mut tile_fetch_ms, &mut tiles_fetched);

        let missing: Vec<TileId> = wanted
            .iter()
            .copied()
            .filter(|id| !self.tiles.contains_key(id))
            .collect();

        let ready_wanted: HashSet<TileId> = wanted_set
            .iter()
            .copied()
            .filter(|id| self.tiles.contains_key(id))
            .collect();
        let wanted_complete = !wanted_set.is_empty() && ready_wanted.len() == wanted_set.len();

        // Sticky only same-zoom tiles while the new footprint is still loading.
        let mut display = ready_wanted.clone();
        let mut sticky_retained = 0u32;
        if !wanted_complete {
            for id in &self.visible {
                if id.z == tile_zoom && self.tiles.contains_key(id) && display.insert(*id) {
                    sticky_retained += 1;
                }
            }
        }

        // Rebase camera-relative mesh when the look-at drifts too far from the
        // uploaded origin. Small pans are absorbed in view_proj residual.
        let needs_rebase = self
            .mesh_origin
            .map(|prev| {
                let dx = prev[0] - origin[0];
                let dz = prev[1] - origin[1];
                dx * dx + dz * dz > MESH_REBASE_DISTANCE * MESH_REBASE_DISTANCE
            })
            .unwrap_or(true);

        if !loaded_any && display == self.visible && !needs_rebase && !pruned_other_zoom {
            return SyncTimings {
                total_ms: elapsed_ms(sync_start),
                tile_fetch_ms,
                merge_ms,
                upload_ms,
                tiles_fetched,
            };
        }
        if display.is_empty() {
            // Still waiting for first tiles — not an error while in-flight.
            if self.data_runtime.in_flight() == 0 && missing.is_empty() {
                log::warn!(
                    "sync: nothing to display (wanted={}, cached={})",
                    wanted_set.len(),
                    self.tiles.len(),
                );
            }
            return SyncTimings {
                total_ms: elapsed_ms(sync_start),
                tile_fetch_ms,
                merge_ms,
                upload_ms,
                tiles_fetched,
            };
        }

        let prev_visible = self.visible.len();
        let bbox = camera.ground_bbox();
        log::info!(
            "sync: cam target=({:.0},{:.0}) dist={:.0} yaw={:.2} pitch={:.2} z={} | wanted={} ready={} missing={} inflight={} sticky={} display={} (prev={}) rebase={needs_rebase} | tiles=[{}] bbox=[{:.0},{:.0}]x[{:.0},{:.0}]",
            origin[0],
            origin[1],
            camera.distance(),
            camera.yaw(),
            camera.pitch(),
            camera.zoom(),
            wanted_set.len(),
            ready_wanted.len(),
            missing.len(),
            self.data_runtime.in_flight(),
            sticky_retained,
            display.len(),
            prev_visible,
            format_tile_ids(&display, 8),
            bbox.x_min,
            bbox.x_max,
            bbox.z_min,
            bbox.z_max,
        );

        let merge_start = Instant::now();
        let tile_refs: Vec<&TileSceneData> = display
            .iter()
            .filter_map(|id| self.tiles.get(id))
            .collect();

        self.merged = merge_tiles_into_scene_relative(&tile_refs, origin);
        self.mesh_origin = Some(origin);
        merge_ms = elapsed_ms(merge_start);
        log::info!(
            "merged {} tiles → {} bg, {} bldg, {} roads (complete={wanted_complete})",
            display.len(),
            count_background(&self.merged),
            count_buildings(&self.merged),
            count_roads(&self.merged),
        );
        print_perf_if(
            merge_ms,
            format!(
                "merge {} tiles: {merge_ms:.2}ms ({} bg, {} bldg, {} roads)",
                display.len(),
                count_background(&self.merged),
                count_buildings(&self.merged),
                count_roads(&self.merged),
            ),
        );

        let upload_start = Instant::now();
        if let Err(err) = self.renderer.upload_tile(&self.merged, device, queue) {
            log::warn!("failed to upload tile geometry: {err:#}");
            return SyncTimings {
                total_ms: elapsed_ms(sync_start),
                tile_fetch_ms,
                merge_ms,
                upload_ms: elapsed_ms(upload_start),
                tiles_fetched,
            };
        }
        upload_ms = elapsed_ms(upload_start);
        print_perf_if(
            upload_ms,
            format!("gpu upload {} tiles: {upload_ms:.2}ms", display.len()),
        );

        self.visible = display;

        SyncTimings {
            total_ms: elapsed_ms(sync_start),
            tile_fetch_ms,
            merge_ms,
            upload_ms,
            tiles_fetched,
        }
    }

    pub fn clear_color(&self) -> wgpu::Color {
        self.renderer.clear_color()
    }
}

fn count_roads(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Roads(roads) = &layer.payload {
                Some(roads.len())
            } else {
                None
            }
        })
        .sum()
}

fn count_buildings(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Buildings(buildings) = &layer.payload {
                Some(buildings.len())
            } else {
                None
            }
        })
        .sum()
}

fn count_background(scene: &TileSceneData) -> usize {
    scene
        .layers
        .iter()
        .filter_map(|layer| {
            if let LayerPayload::Background(background) = &layer.payload {
                Some(background.len())
            } else {
                None
            }
        })
        .sum()
}
