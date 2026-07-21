use std::fmt::Display;
use std::time::{Duration, Instant};

/// Minimum elapsed time before a perf line is emitted.
pub const PERF_LOG_THRESHOLD_MS: f64 = 1.0;

/// Minimum interactive frames before reporting FPS (avoids nonsense on first frame).
const MIN_INTERACTIVE_FRAMES_FOR_FPS: u64 = 30;

pub fn print_perf(message: impl Display) {
    log::info!("[perf] {message}");
}

pub fn print_perf_if(elapsed_ms: f64, message: impl Display) {
    if elapsed_ms >= PERF_LOG_THRESHOLD_MS {
        print_perf(message);
    }
}

pub fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// Accumulates frame timings and prints summaries every `report_every`.
///
/// Tile-fetch frames are tracked separately from interactive frames so a 6 s
/// network stall does not produce `fps=0.2`.
pub struct FramePerfMonitor {
    frame_start: Instant,
    last_report: Instant,
    report_every: Duration,
    interactive_frames: u64,
    interactive_render_ms: f64,
    load_frames: u64,
    load_fetch_ms: f64,
    load_merge_ms: f64,
    load_upload_ms: f64,
    tiles_fetched: u64,
    last_draw_calls: u32,
    last_triangles: u32,
}

impl FramePerfMonitor {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            frame_start: now,
            last_report: now,
            report_every: Duration::from_secs(2),
            interactive_frames: 0,
            interactive_render_ms: 0.0,
            load_frames: 0,
            load_fetch_ms: 0.0,
            load_merge_ms: 0.0,
            load_upload_ms: 0.0,
            tiles_fetched: 0,
            last_draw_calls: 0,
            last_triangles: 0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.frame_start = Instant::now();
    }

    pub fn end_frame(
        &mut self,
        tile_fetch_ms: f64,
        merge_ms: f64,
        upload_ms: f64,
        tiles_fetched: u32,
        render_ms: f64,
        draw_calls: u32,
        triangles: u32,
    ) {
        self.last_draw_calls = draw_calls;
        self.last_triangles = triangles;

        let is_load_frame =
            tiles_fetched > 0 || tile_fetch_ms >= PERF_LOG_THRESHOLD_MS;

        if is_load_frame {
            self.load_frames += 1;
            self.load_fetch_ms += tile_fetch_ms;
            self.load_merge_ms += merge_ms;
            self.load_upload_ms += upload_ms;
            self.tiles_fetched += u64::from(tiles_fetched);

            let load_ms = tile_fetch_ms + merge_ms + upload_ms;
            print_perf_if(
                load_ms,
                format!(
                    "tile load (blocked): fetch={tile_fetch_ms:.0}ms merge={merge_ms:.1}ms upload={upload_ms:.1}ms tiles={tiles_fetched} render={render_ms:.1}ms draw_calls={draw_calls} triangles={triangles}",
                ),
            );
        } else {
            self.interactive_frames += 1;
            self.interactive_render_ms += render_ms;
        }

        if self.last_report.elapsed() >= self.report_every {
            self.print_summary();
            self.reset_window();
        }
    }

    fn print_summary(&self) {
        if self.load_frames > 0 && self.load_fetch_ms >= PERF_LOG_THRESHOLD_MS {
            print_perf(format!(
                "tile load window: fetch={:.0}ms merge={:.1}ms upload={:.1}ms tiles={} ({} blocked frames)",
                self.load_fetch_ms,
                self.load_merge_ms,
                self.load_upload_ms,
                self.tiles_fetched,
                self.load_frames,
            ));
        }

        if self.interactive_frames >= MIN_INTERACTIVE_FRAMES_FOR_FPS {
            let avg_render = self.interactive_render_ms / self.interactive_frames as f64;
            let fps = if avg_render > 0.0 {
                1000.0 / avg_render
            } else {
                0.0
            };
            print_perf(format!(
                "fps={fps:.1} render={avg_render:.2}ms draw_calls={} triangles={} ({} interactive frames)",
                self.last_draw_calls,
                self.last_triangles,
                self.interactive_frames,
            ));
        }
    }

    fn reset_window(&mut self) {
        self.last_report = Instant::now();
        self.interactive_frames = 0;
        self.interactive_render_ms = 0.0;
        self.load_frames = 0;
        self.load_fetch_ms = 0.0;
        self.load_merge_ms = 0.0;
        self.load_upload_ms = 0.0;
        self.tiles_fetched = 0;
    }
}

impl Default for FramePerfMonitor {
    fn default() -> Self {
        Self::new()
    }
}
