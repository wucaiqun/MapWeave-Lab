use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mw_core::{TileId, TileSceneData};
use mw_provider_mvt::{MvtProvider, MvtProviderConfig, TileProvider};
use tokio::sync::{mpsc, Semaphore};

use crate::DEFAULT_TILE_FETCH_CONCURRENCY;

/// Unit of async data work submitted from the main thread.
#[derive(Debug)]
pub enum DataJob {
    /// Resolve TileJSON / endpoint once.
    ProviderInit { config: MvtProviderConfig },
    /// Download + decode + map one MVT tile (contains Background/Roads/Buildings).
    TileFetch { tile_id: TileId, priority: u64 },
}

/// Results drained by the main thread (never block the frame loop waiting).
#[derive(Debug)]
pub enum DataResult {
    ProviderReady { endpoint: String },
    ProviderFailed { error: String },
    TileFetched {
        tile_id: TileId,
        scene: TileSceneData,
        elapsed_ms: f64,
    },
    TileFailed { tile_id: TileId, error: String },
}

enum WorkerMsg {
    Job(DataJob),
    Shutdown,
}

/// Tokio-backed job runtime shared by all async data kinds.
pub struct DataTaskRuntime {
    _runtime: tokio::runtime::Runtime,
    job_tx: mpsc::UnboundedSender<WorkerMsg>,
    result_rx: Mutex<std::sync::mpsc::Receiver<DataResult>>,
    /// Tile ids currently accepted / running (shared with worker for dedupe).
    in_flight_tiles: Arc<Mutex<HashSet<TileId>>>,
    in_flight_count: Arc<AtomicUsize>,
}

impl DataTaskRuntime {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_tile_concurrency(DEFAULT_TILE_FETCH_CONCURRENCY)
    }

    pub fn with_tile_concurrency(tile_concurrency: usize) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("mw-async-data")
            .build()?;

        let (job_tx, job_rx) = mpsc::unbounded_channel::<WorkerMsg>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<DataResult>();
        let in_flight_tiles = Arc::new(Mutex::new(HashSet::new()));
        let in_flight_count = Arc::new(AtomicUsize::new(0));

        let tile_cap = Arc::new(Semaphore::new(tile_concurrency.max(1)));

        runtime.spawn(worker_loop(job_rx, result_tx, tile_cap));

        Ok(Self {
            _runtime: runtime,
            job_tx,
            result_rx: Mutex::new(result_rx),
            in_flight_tiles,
            in_flight_count,
        })
    }

    pub fn submit(&self, job: DataJob) {
        match &job {
            DataJob::TileFetch { tile_id, .. } => {
                let mut guard = self
                    .in_flight_tiles
                    .lock()
                    .expect("in_flight_tiles lock");
                if !guard.insert(*tile_id) {
                    return; // already queued or running
                }
                self.in_flight_count.fetch_add(1, Ordering::Relaxed);
            }
            DataJob::ProviderInit { .. } => {}
        }

        if let Err(err) = self.job_tx.send(WorkerMsg::Job(job)) {
            // Roll back in-flight bookkeeping if the worker is gone.
            if let WorkerMsg::Job(DataJob::TileFetch { tile_id, .. }) = err.0 {
                let mut guard = self
                    .in_flight_tiles
                    .lock()
                    .expect("in_flight_tiles lock");
                guard.remove(&tile_id);
                self.in_flight_count.fetch_sub(1, Ordering::Relaxed);
            }
            log::warn!("data-task runtime: job channel closed");
        }
    }

    pub fn submit_many(&self, jobs: impl IntoIterator<Item = DataJob>) {
        for job in jobs {
            self.submit(job);
        }
    }

    /// Non-blocking drain of all ready results.
    pub fn drain(&self) -> Vec<DataResult> {
        let rx = self.result_rx.lock().expect("result_rx lock");
        let mut out = Vec::new();
        while let Ok(result) = rx.try_recv() {
            if let DataResult::TileFetched { tile_id, .. }
            | DataResult::TileFailed { tile_id, .. } = &result
            {
                let mut guard = self
                    .in_flight_tiles
                    .lock()
                    .expect("in_flight_tiles lock");
                guard.remove(tile_id);
                self.in_flight_count.fetch_sub(1, Ordering::Relaxed);
            }
            out.push(result);
        }
        out
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight_count.load(Ordering::Relaxed)
    }

    pub fn is_tile_in_flight(&self, tile_id: TileId) -> bool {
        self.in_flight_tiles
            .lock()
            .expect("in_flight_tiles lock")
            .contains(&tile_id)
    }
}

impl Drop for DataTaskRuntime {
    fn drop(&mut self) {
        let _ = self.job_tx.send(WorkerMsg::Shutdown);
    }
}

async fn worker_loop(
    mut job_rx: mpsc::UnboundedReceiver<WorkerMsg>,
    result_tx: std::sync::mpsc::Sender<DataResult>,
    tile_cap: Arc<Semaphore>,
) {
    let mut provider: Option<Arc<MvtProvider>> = None;

    while let Some(msg) = job_rx.recv().await {
        match msg {
            WorkerMsg::Shutdown => break,
            WorkerMsg::Job(DataJob::ProviderInit { config }) => {
                match MvtProvider::with_resolved_config(config).await {
                    Ok(p) => {
                        let endpoint = p.config.endpoint_template.clone();
                        provider = Some(Arc::new(p));
                        let _ = result_tx.send(DataResult::ProviderReady { endpoint });
                    }
                    Err(err) => {
                        let _ = result_tx.send(DataResult::ProviderFailed {
                            error: format!("{err:#}"),
                        });
                    }
                }
            }
            WorkerMsg::Job(DataJob::TileFetch { tile_id, priority: _ }) => {
                let Some(provider) = provider.clone() else {
                    // Main thread clears in-flight when draining this failure.
                    let _ = result_tx.send(DataResult::TileFailed {
                        tile_id,
                        error: "provider not ready".to_string(),
                    });
                    continue;
                };

                let permit = match tile_cap.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        let _ = result_tx.send(DataResult::TileFailed {
                            tile_id,
                            error: "tile semaphore closed".to_string(),
                        });
                        continue;
                    }
                };

                let result_tx = result_tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let start = Instant::now();
                    match provider.fetch_tile(tile_id).await {
                        Ok(scene) => {
                            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                            let _ = result_tx.send(DataResult::TileFetched {
                                tile_id,
                                scene,
                                elapsed_ms,
                            });
                        }
                        Err(err) => {
                            let _ = result_tx.send(DataResult::TileFailed {
                                tile_id,
                                error: format!("{err:#}"),
                            });
                        }
                    }
                });
            }
        }
    }
}
