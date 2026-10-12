use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::env::{MACOS, data_dir, jobs_dir};
use super::host::{host_name, pid_start};
use super::util::{self, Clock, atomic_write, filled};

const CAP: usize = 2048;
const ONE_US: u64 = 1_000_000;
const FIVE_US: u64 = 5_000_000;
const FILE_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Tui,
    Desk,
    Service,
}

impl Surface {
    fn as_str(self) -> &'static str {
        match self {
            Surface::Tui => "tui",
            Surface::Desk => "desk",
            Surface::Service => "service",
        }
    }

    fn target_fps(self) -> f64 {
        match self {
            Surface::Tui => 120.0,
            Surface::Desk => 60.0,
            Surface::Service => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pacing {
    Continuous,
    OnChange,
}

impl Pacing {
    fn as_str(self) -> &'static str {
        match self {
            Pacing::Continuous => "continuous",
            Pacing::OnChange => "on_change",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Span {
    pub p50: f64,
    #[serde(default)]
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    #[serde(default)]
    pub v: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub pid_start: u64,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub ride: bool,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub frame_ms: Option<Span>,
    #[serde(default)]
    pub gpu_ms: Option<Span>,
    #[serde(default)]
    pub interval_ms: Option<Span>,
    #[serde(default)]
    pub missed: u64,
    #[serde(default)]
    pub target_fps: f64,
    #[serde(default)]
    pub pacing: String,
    #[serde(default)]
    pub idle: bool,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_pass_ms: Option<BTreeMap<String, f64>>,
    #[serde(default)]
    pub ride_set: Option<String>,
    #[serde(default)]
    pub started: u64,
    #[serde(default)]
    pub updated: u64,
}

pub fn live(app: &str, version: &str, surface: Surface, pacing: Pacing) -> Handle {
    open(
        app,
        version,
        surface,
        pacing,
        Env::from_env(),
        Clock::system(),
        true,
    )
}

pub fn apps_dir() -> Option<PathBuf> {
    Env::from_env().dir
}

pub fn read_apps(path: impl AsRef<Path>) -> Vec<Beat> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(bytes) = crate::util::read_capped(&entry.path()) else {
            continue;
        };
        let Ok(beat) = serde_json::from_slice::<Beat>(&bytes) else {
            continue;
        };
        if beat.app.is_empty() {
            continue;
        }
        found.push((name.into_owned(), beat));
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found.into_iter().map(|(_, beat)| beat).collect()
}

#[must_use = "dropping the handle removes the heartbeat file"]
pub struct Handle {
    inner: Option<Arc<Inner>>,
    stop: Option<Arc<AtomicBool>>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn frame(&self, work: Duration) {
        self.push(false, work, None);
    }

    pub fn frame_with(&self, cpu: Duration, gpu: Option<Duration>) {
        self.push(false, cpu, gpu);
    }

    pub fn frame_idle(&self) {
        self.push(true, Duration::ZERO, None);
    }

    pub fn detail(&self, text: &str) {
        let Some(inner) = &self.inner else {
            return;
        };
        let line = one_line(text, 240);
        let mut slot = inner.detail.lock().unwrap_or_else(|err| err.into_inner());
        *slot = if line.is_empty() { None } else { Some(line) };
    }

    pub fn size(&self, w: u32, h: u32) {
        if let Some(inner) = &self.inner {
            *inner.size.lock().unwrap_or_else(|err| err.into_inner()) = Some([w, h]);
        }
    }

    pub fn renderer(&self, name: &str) {
        if let Some(inner) = &self.inner {
            let name = short_line(name, 16);
            *inner.renderer.lock().unwrap_or_else(|err| err.into_inner()) =
                (!name.is_empty()).then_some(name);
        }
    }

    pub fn passes(&self, passes: &[(&str, f64)]) {
        if let Some(inner) = &self.inner {
            let mut values = BTreeMap::new();
            for &(name, ms) in passes {
                if values.len() >= 16 {
                    break;
                }
                if !ms.is_finite() || ms < 0.0 {
                    continue;
                }
                let name = short_line(name, 24);
                if !name.is_empty() {
                    values.insert(name, ms);
                }
            }
            *inner.passes.lock().unwrap_or_else(|err| err.into_inner()) = Some(values);
        }
    }

    fn push(&self, idle: bool, cpu: Duration, gpu: Option<Duration>) {
        let Some(inner) = &self.inner else {
            return;
        };
        let seq = inner.next.fetch_add(1, Ordering::Relaxed);
        let slot = &inner.slots[(seq as usize) % CAP];
        slot.at.store(inner.clock.mono_us(), Ordering::Relaxed);
        slot.cpu.store(as_ns(cpu), Ordering::Relaxed);
        slot.gpu
            .store(gpu.map(as_ns).unwrap_or(u64::MAX), Ordering::Relaxed);
        slot.flags.store(u64::from(idle), Ordering::Relaxed);
        slot.seq.store(seq + 1, Ordering::Release);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Release);
        }
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        if let Some(inner) = &self.inner {
            inner.remove_file();
        }
    }
}

struct Slot {
    seq: AtomicU64,
    at: AtomicU64,
    cpu: AtomicU64,
    gpu: AtomicU64,
    flags: AtomicU64,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            seq: AtomicU64::new(0),
            at: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            gpu: AtomicU64::new(0),
            flags: AtomicU64::new(0),
        }
    }
}

struct Mark {
    at_us: u64,
    cpu_ns: u64,
    gpu_ns: Option<u64>,
    idle: bool,
}

struct Accum {
    cursor: u64,
    marks: Vec<Mark>,
}

struct Measured {
    fps: f64,
    idle: bool,
    frame: Option<Span>,
    gpu: Option<Span>,
    interval: Option<Span>,
    missed: u64,
}

struct Inner {
    clock: Clock,
    slots: Vec<Slot>,
    next: AtomicU64,
    accum: Mutex<Accum>,
    detail: Mutex<Option<String>>,
    size: Mutex<Option<[u32; 2]>>,
    renderer: Mutex<Option<String>>,
    passes: Mutex<Option<BTreeMap<String, f64>>>,
    wrote: AtomicBool,
    last_us: AtomicU64,
    dir: PathBuf,
    file: String,
    app: String,
    version: String,
    surface: Surface,
    pacing: Pacing,
    target_fps: f64,
    id: String,
    host: String,
    pid: u32,
    pid_start: u64,
    ride: bool,
    ride_set: Option<String>,
    started_ms: u64,
}

impl Inner {
    fn write_if_due(&self) {
        let now = self.clock.mono_us();
        self.collect(now);
        if self.wrote.load(Ordering::Relaxed) {
            let last = self.last_us.load(Ordering::Relaxed);
            if now.saturating_sub(last) < ONE_US {
                return;
            }
        }
        let measured = self.measured(now);
        let detail = self
            .detail
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let size = *self.size.lock().unwrap_or_else(|err| err.into_inner());
        let renderer = self
            .renderer
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let passes = self
            .passes
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let beat = self.beat(&measured, detail, size, renderer, passes);
        let bytes = encode(&beat);
        if bytes.is_empty() {
            return;
        }
        if atomic_write(&self.dir, &self.file, &bytes).is_ok() {
            self.last_us.store(now, Ordering::Relaxed);
            self.wrote.store(true, Ordering::Relaxed);
        }
    }

    fn collect(&self, now: u64) {
        let mut accum = self.accum.lock().unwrap_or_else(|err| err.into_inner());
        drain(&self.slots, &mut accum);
        prune(&mut accum.marks, now);
    }

    fn measured(&self, now: u64) -> Measured {
        let accum = self.accum.lock().unwrap_or_else(|err| err.into_inner());
        let target = match self.pacing {
            Pacing::Continuous => self.target_fps,
            Pacing::OnChange => 0.0,
        };
        measure(&accum.marks, now, target)
    }

    fn beat(
        &self,
        measured: &Measured,
        detail: Option<String>,
        size: Option<[u32; 2]>,
        renderer: Option<String>,
        gpu_pass_ms: Option<BTreeMap<String, f64>>,
    ) -> Beat {
        Beat {
            v: 1,
            id: self.id.clone(),
            host: self.host.clone(),
            pid: self.pid,
            pid_start: self.pid_start,
            app: self.app.clone(),
            version: self.version.clone(),
            surface: self.surface.as_str().to_string(),
            ride: self.ride,
            fps: measured.fps,
            frame_ms: measured.frame.clone(),
            gpu_ms: measured.gpu.clone(),
            interval_ms: measured.interval.clone(),
            missed: measured.missed,
            target_fps: self.target_fps,
            pacing: self.pacing.as_str().to_string(),
            idle: measured.idle,
            detail,
            size,
            renderer,
            gpu_pass_ms,
            ride_set: self.ride_set.clone(),
            started: self.started_ms,
            updated: self.clock.wall_ms(),
        }
    }

    fn remove_file(&self) {
        let _ = std::fs::remove_file(self.dir.join(&self.file));
    }
}

fn open(
    app: &str,
    version: &str,
    surface: Surface,
    pacing: Pacing,
    env: Env,
    clock: Clock,
    spawn: bool,
) -> Handle {
    let Some(dir) = env.dir else {
        return Handle {
            inner: None,
            stop: None,
            thread: None,
        };
    };
    if env.off || !util::WRITES {
        return Handle {
            inner: None,
            stop: None,
            thread: None,
        };
    }
    let started_ms = clock.wall_ms();
    let pid = util::process_id();
    let host = host_name();
    let file = format!("{pid}-{started_ms}.json");
    let inner = Arc::new(Inner {
        clock,
        slots: (0..CAP).map(|_| Slot::new()).collect(),
        next: AtomicU64::new(0),
        accum: Mutex::new(Accum {
            cursor: 0,
            marks: Vec::new(),
        }),
        detail: Mutex::new(None),
        size: Mutex::new(None),
        renderer: Mutex::new(None),
        passes: Mutex::new(None),
        wrote: AtomicBool::new(false),
        last_us: AtomicU64::new(0),
        dir,
        file,
        app: one_line(app, 128),
        version: one_line(version, 128),
        surface,
        pacing,
        target_fps: surface.target_fps(),
        id: format!("{host}:{pid}:{started_ms}"),
        host,
        pid,
        pid_start: pid_start(),
        ride: env.ride,
        ride_set: env.ride_set.map(|set| one_line(&set, 128)),
        started_ms,
    });
    if !spawn {
        return Handle {
            inner: Some(inner),
            stop: None,
            thread: None,
        };
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let inner = Arc::clone(&inner);
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("pito-live".to_string())
            .spawn(move || worker(inner, stop))
            .ok()
    };
    Handle {
        inner: Some(inner),
        stop: Some(stop),
        thread,
    }
}

fn worker(inner: Arc<Inner>, stop: Arc<AtomicBool>) {
    be_nice();
    while !stop.load(Ordering::Acquire) {
        inner.write_if_due();
        if stop.load(Ordering::Acquire) {
            break;
        }
        thread::park_timeout(Duration::from_millis(200));
    }
    inner.remove_file();
}

#[cfg(target_os = "linux")]
fn be_nice() {
    unsafe {
        let tid = libc::gettid();
        libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, 19);
        libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13);
    }
}

#[cfg(not(target_os = "linux"))]
fn be_nice() {}

fn drain(slots: &[Slot], accum: &mut Accum) {
    for _ in 0..CAP.saturating_mul(2) {
        let slot = &slots[(accum.cursor as usize) % CAP];
        let seq = slot.seq.load(Ordering::Acquire);
        let expect = accum.cursor + 1;
        if seq == expect {
            let at = slot.at.load(Ordering::Relaxed);
            let cpu = slot.cpu.load(Ordering::Relaxed);
            let gpu = slot.gpu.load(Ordering::Relaxed);
            let flags = slot.flags.load(Ordering::Relaxed);
            if slot.seq.load(Ordering::Acquire) != expect {
                break;
            }
            accum.marks.push(Mark {
                at_us: at,
                cpu_ns: cpu,
                gpu_ns: (gpu != u64::MAX).then_some(gpu),
                idle: flags == 1,
            });
            accum.cursor += 1;
            continue;
        }
        if seq > expect {
            accum.cursor = seq - 1;
            continue;
        }
        break;
    }
}

fn prune(marks: &mut Vec<Mark>, now: u64) {
    if let Some(index) = marks
        .iter()
        .position(|mark| now.saturating_sub(mark.at_us) <= FIVE_US)
    {
        let drop_n = index.saturating_sub(1);
        if drop_n > 0 {
            marks.drain(..drop_n);
        }
    } else if marks.len() > 1 {
        let last = marks.pop().unwrap();
        marks.clear();
        marks.push(last);
    }
    if marks.len() > 8192 {
        let drop_n = marks.len() - 8192;
        marks.drain(..drop_n);
    }
}

fn measure(marks: &[Mark], now: u64, target_fps: f64) -> Measured {
    let mut frames = Vec::new();
    let mut gpus = Vec::new();
    let mut gaps = Vec::new();
    let mut fps = 0u64;
    let mut missed = 0u64;
    let mut prev: Option<u64> = None;
    let limit = if target_fps > 0.0 {
        1.5 * (ONE_US as f64 / target_fps)
    } else {
        0.0
    };
    for mark in marks {
        if mark.idle {
            continue;
        }
        let age = now.saturating_sub(mark.at_us);
        let in_five = age <= FIVE_US;
        if age <= ONE_US {
            fps += 1;
        }
        if let Some(before) = prev
            && in_five
        {
            let gap = mark.at_us.saturating_sub(before);
            gaps.push(gap as f64 / 1000.0);
            if limit > 0.0 && gap as f64 > limit {
                missed += 1;
            }
        }
        prev = Some(mark.at_us);
        if in_five {
            frames.push(mark.cpu_ns as f64 / 1_000_000.0);
            if let Some(gpu) = mark.gpu_ns {
                gpus.push(gpu as f64 / 1_000_000.0);
            }
        }
    }
    Measured {
        fps: fps as f64,
        idle: fps == 0,
        frame: span_of(&frames),
        gpu: span_of(&gpus),
        interval: span_of(&gaps),
        missed,
    }
}

fn span_of(values: &[f64]) -> Option<Span> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    Some(Span {
        p50: round1(percentile(&sorted, 0.50)),
        p95: round1(percentile(&sorted, 0.95)),
        p99: round1(percentile(&sorted, 0.99)),
        max: round1(sorted[sorted.len() - 1]),
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let idx = ((p * n as f64).ceil() as usize)
        .saturating_sub(1)
        .min(n - 1);
    sorted[idx]
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn encode(beat: &Beat) -> Vec<u8> {
    let Ok(bytes) = serde_json::to_vec(beat) else {
        return Vec::new();
    };
    if bytes.len() <= FILE_LIMIT {
        return bytes;
    }
    let mut copy = beat.clone();
    copy.detail = None;
    match serde_json::to_vec(&copy) {
        Ok(bytes) if bytes.len() <= FILE_LIMIT => bytes,
        _ => Vec::new(),
    }
}

fn as_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn one_line(text: &str, max: usize) -> String {
    let text = text.split(['\n', '\r']).next().unwrap_or("");
    let mut end = text.len().min(max);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn short_line(text: &str, max: usize) -> String {
    text.chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .filter(|ch| !ch.is_control())
        .take(max)
        .collect()
}

#[derive(Clone, Debug)]
struct Env {
    dir: Option<PathBuf>,
    off: bool,
    ride: bool,
    ride_set: Option<String>,
}

impl Env {
    fn from_env() -> Env {
        Self::from_fn(|key| std::env::var(key).ok(), |path| path.is_dir())
    }

    fn from_fn(mut get: impl FnMut(&str) -> Option<String>, exists: impl Fn(&Path) -> bool) -> Env {
        let asked_off = get("PITO_JOBS")
            .as_deref()
            .is_some_and(|value| value.trim() == "off");
        let data = data_dir(&mut get, MACOS);
        let installed = filled(get("PITO_JOBS_DIR")).is_some()
            || data.is_some_and(|data| exists(&data.join("pito").join("apps")));
        let off = asked_off || !installed;
        let ride_app = filled(get("RIDE_APP"));
        let ride_set = filled(get("RIDE_SET"));
        let dir = jobs_dir(&mut get, MACOS).map(|dir| dir.join("apps"));
        Env {
            dir,
            off,
            ride: ride_app.is_some(),
            ride_set,
        }
    }
}

#[cfg(test)]
impl Handle {
    fn drive(&self) {
        if let Some(inner) = &self.inner {
            inner.write_if_due();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_apps_skips_a_file_over_four_kilobytes_and_keeps_one_at_the_cap() {
        let dir = scratch("apps-cap");
        let kept = capped_beat(crate::util::READ_CAP);
        let mut over = kept.clone();
        over.push(b' ');
        std::fs::write(dir.join("kept.json"), &kept).unwrap();
        std::fs::write(dir.join("over.json"), &over).unwrap();
        let beats = read_apps(&dir);
        assert_eq!(beats.len(), 1);
        assert_eq!(beats[0].app, "paper");
        assert_eq!(kept.len(), crate::util::READ_CAP);
    }

    fn capped_beat(len: usize) -> Vec<u8> {
        let prefix = br#"{"app":"paper","id":""#;
        let suffix = br#""}"#;
        let pad = len - prefix.len() - suffix.len();
        let mut bytes = Vec::with_capacity(len);
        bytes.extend_from_slice(prefix);
        bytes.extend(std::iter::repeat_n(b'a', pad));
        bytes.extend_from_slice(suffix);
        assert_eq!(bytes.len(), len);
        bytes
    }

    fn scratch(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn env_at(dir: &Path) -> Env {
        Env {
            dir: Some(dir.to_path_buf()),
            off: false,
            ride: false,
            ride_set: None,
        }
    }

    fn mark(at_ms: u64, cpu_ms: f64, gpu_ms: Option<f64>, idle: bool) -> Mark {
        Mark {
            at_us: at_ms * 1000,
            cpu_ns: (cpu_ms * 1_000_000.0).round() as u64,
            gpu_ns: gpu_ms.map(|ms| (ms * 1_000_000.0).round() as u64),
            idle,
        }
    }

    #[test]
    fn percentiles_use_the_nearest_rank_over_the_sorted_window() {
        let values: Vec<f64> = (1..=100).map(|n| n as f64).collect();
        assert_eq!(percentile(&values, 0.50), 50.0);
        assert_eq!(percentile(&values, 0.95), 95.0);
        assert_eq!(percentile(&values, 0.99), 99.0);
        assert_eq!(percentile(&values, 0.50), span_of(&values).unwrap().p50);
        assert_eq!(span_of(&values).unwrap().p95, 95.0);
        let one = span_of(&[4.25]).unwrap();
        assert_eq!(one.p50, 4.3);
        assert_eq!(one.p95, 4.3);
        assert_eq!(one.p99, 4.3);
        assert_eq!(one.max, 4.3);
        assert!(span_of(&[]).is_none());
    }

    #[test]
    fn fps_is_the_last_second_and_percentiles_the_last_five() {
        let mut marks = Vec::new();
        for i in 0..100 {
            let at = 5_000 + i * 50;
            let gpu = (i % 2 == 0).then_some(i as f64 + 0.5);
            marks.push(mark(at, (i + 1) as f64, gpu, false));
        }
        marks.push(mark(9_900, 0.1, None, true));
        let now = 10_000_000u64;
        let got = measure(&marks, now, 120.0);
        assert_eq!(got.fps, 20.0);
        assert!(!got.idle);
        let frame = got.frame.unwrap();
        assert_eq!(frame.p50, 50.0);
        assert_eq!(frame.p95, 95.0);
        assert_eq!(frame.p99, 99.0);
        assert_eq!(frame.max, 100.0);
        let gpu = got.gpu.unwrap();
        assert_eq!(gpu.max, 98.5);
        assert_eq!(gpu.p95, 94.5);
        let interval = got.interval.unwrap();
        assert_eq!(interval.p50, 50.0);
        assert_eq!(interval.p95, 50.0);
        assert_eq!(interval.max, 50.0);
        assert_eq!(got.missed, 99);
    }

    #[test]
    fn idle_ticks_do_not_count_and_a_short_gap_is_not_missed() {
        let marks = vec![
            mark(0, 1.0, None, false),
            mark(8, 1.0, None, true),
            mark(16, 2.0, Some(3.0), false),
        ];
        let got = measure(&marks, 20_000, 60.0);
        assert_eq!(got.fps, 2.0);
        assert!(!got.idle);
        assert_eq!(got.interval.unwrap().p50, 16.0);
        assert_eq!(got.missed, 0);
        assert_eq!(got.gpu.unwrap().p99, 3.0);
        let quiet = measure(&[mark(0, 1.0, None, true)], 2_000_000, 120.0);
        assert_eq!(quiet.fps, 0.0);
        assert!(quiet.idle);
        assert!(quiet.frame.is_none());
        assert_eq!(quiet.missed, 0);
        let no_target = measure(&marks, 20_000, 0.0);
        assert_eq!(no_target.missed, 0);
    }

    #[test]
    fn an_old_anchor_still_opens_the_first_interval() {
        let marks = vec![mark(0, 9.0, None, false), mark(6_000, 1.0, None, false)];
        let got = measure(&marks, 6_000_000, 120.0);
        assert_eq!(got.fps, 1.0);
        assert_eq!(got.frame.unwrap().p50, 1.0);
        assert_eq!(got.interval.unwrap().max, 6000.0);
        assert_eq!(got.missed, 1);
    }

    #[test]
    fn the_folder_follows_the_environment_and_off_clears_nothing_about_it() {
        let off = Env::from_fn(
            |key| match key {
                "PITO_JOBS" => Some(" off ".to_string()),
                "PITO_JOBS_DIR" => Some("/custom".to_string()),
                "RIDE_APP" => Some(" paper ".to_string()),
                "RIDE_SET" => Some(" desk-a ".to_string()),
                _ => None,
            },
            |_| true,
        );
        assert!(off.off);
        assert_eq!(off.dir.unwrap(), PathBuf::from("/custom/apps"));
        assert!(off.ride);
        assert_eq!(off.ride_set.as_deref(), Some("desk-a"));
        let xdg = Env::from_fn(
            |key| match key {
                "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
                "HOME" => Some("/u".to_string()),
                "RIDE_APP" => Some("  ".to_string()),
                _ => None,
            },
            |path| {
                let data = if MACOS {
                    "/u/Library/Application Support"
                } else {
                    "/u/.local/share"
                };
                path == Path::new(data).join("pito").join("apps")
            },
        );
        assert!(!xdg.off);
        assert!(!xdg.ride);
        assert_eq!(
            xdg.dir.unwrap(),
            PathBuf::from("/run/user/1000/pito-jobs/apps")
        );
        let missing = Env::from_fn(|_| None, |_| false);
        assert!(missing.dir.is_none());
    }

    #[test]
    fn a_buyers_machine_gets_an_inert_handle() {
        let runtime = |key: &str| match key {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
            "HOME" => Some("/buyer".to_string()),
            _ => None,
        };
        assert!(Env::from_fn(runtime, |_| false).off);
        let data_home = Env::from_fn(
            |key| match key {
                "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
                "XDG_DATA_HOME" => Some("/data".to_string()),
                _ => None,
            },
            |path| path == Path::new("/data/pito/apps"),
        );
        assert!(!data_home.off);
        let pinned = Env::from_fn(
            |key| match key {
                "PITO_JOBS_DIR" => Some("/tmp-under-test".to_string()),
                _ => None,
            },
            |_| false,
        );
        assert!(!pinned.off);
    }

    #[test]
    fn writes_wait_a_second_and_off_writes_nothing() {
        let dir = scratch("live-throttle");
        let (clock, us) = Clock::manual(10_000_000);
        let handle = open(
            "paper",
            "0.12.0",
            Surface::Tui,
            Pacing::OnChange,
            env_at(&dir),
            clock,
            false,
        );
        handle.frame(Duration::from_millis(2));
        handle.drive();
        let first = read_apps(&dir);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].fps, 1.0);
        assert_eq!(first[0].updated, 10_000);
        us.store(10_000_000 + 999_000, Ordering::Relaxed);
        handle.frame(Duration::from_millis(2));
        handle.drive();
        assert_eq!(read_apps(&dir)[0].fps, 1.0);
        us.store(10_000_000 + 1_000_000, Ordering::Relaxed);
        handle.drive();
        let second = read_apps(&dir);
        assert_eq!(second[0].fps, 2.0);
        assert_eq!(second[0].updated, 11_000);
        drop(handle);

        let quiet = scratch("live-off");
        let (clock, _) = Clock::manual(1_000);
        let mut cfg = env_at(&quiet);
        cfg.off = true;
        let handle = open(
            "paper",
            "0.1.0",
            Surface::Desk,
            Pacing::Continuous,
            cfg,
            clock,
            false,
        );
        handle.frame(Duration::from_millis(1));
        handle.frame_idle();
        handle.detail("nope");
        handle.size(3840, 2160);
        handle.renderer("live");
        handle.passes(&[("shadow", 1.4)]);
        handle.drive();
        assert!(read_apps(&quiet).is_empty());
    }

    #[test]
    fn the_file_round_trips_ride_detail_and_drop_removes_it() {
        let dir = scratch("live-round");
        let (clock, us) = Clock::manual(2_000_000);
        let mut cfg = env_at(&dir);
        cfg.ride = true;
        cfg.ride_set = Some("desk-a".to_string());
        let handle = open(
            "studio",
            "0.4.0",
            Surface::Desk,
            Pacing::Continuous,
            cfg,
            clock,
            false,
        );
        for i in 0..5 {
            us.store(2_000_000 + i * 16_000, Ordering::Relaxed);
            handle.frame_with(
                Duration::from_micros(1_500),
                Some(Duration::from_micros(4_000)),
            );
        }
        handle.frame_idle();
        handle.detail("converging 128/512 spp\nsecond");
        handle.size(3840, 2160);
        handle.renderer("live\nignored");
        handle.passes(&[("shadow", 1.4), ("opaque", 3.9)]);
        handle.drive();
        let beat = read_apps(&dir).pop().unwrap();
        assert_eq!(beat.v, 1);
        assert_eq!(beat.app, "studio");
        assert_eq!(beat.version, "0.4.0");
        assert_eq!(beat.surface, "desk");
        assert_eq!(beat.pacing, "continuous");
        assert!(beat.ride);
        assert_eq!(beat.ride_set.as_deref(), Some("desk-a"));
        assert_eq!(beat.detail.as_deref(), Some("converging 128/512 spp"));
        assert_eq!(beat.size, Some([3840, 2160]));
        assert_eq!(beat.renderer.as_deref(), Some("live"));
        assert_eq!(beat.gpu_pass_ms.as_ref().unwrap()["shadow"], 1.4);
        assert_eq!(beat.fps, 5.0);
        assert!(!beat.idle);
        assert_eq!(beat.target_fps, 60.0);
        assert_eq!(beat.frame_ms.unwrap().p50, 1.5);
        assert_eq!(beat.gpu_ms.unwrap().p99, 4.0);
        assert_eq!(beat.interval_ms.unwrap().p50, 16.0);
        assert_eq!(beat.missed, 0);
        assert!(beat.id.contains(&beat.pid.to_string()));
        let raw = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .find(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
            .unwrap();
        assert!(
            raw.file_name()
                .to_string_lossy()
                .starts_with(&format!("{}-", beat.pid))
        );
        drop(handle);
        assert!(read_apps(&dir).is_empty());
    }

    #[test]
    fn passes_drop_invalid_values_and_cap_names_and_count() {
        let dir = scratch("live-passes");
        let (clock, us) = Clock::manual(3_000_000);
        let handle = open(
            "studio",
            "0.4.0",
            Surface::Desk,
            Pacing::Continuous,
            env_at(&dir),
            clock,
            false,
        );
        let names: Vec<_> = (0..17).map(|i| format!("pass{i:02}")).collect();
        let mut input: Vec<_> = names.iter().map(|name| (name.as_str(), 1.0)).collect();
        let long = "x".repeat(40);
        input.insert(0, (long.as_str(), 2.0));
        input.insert(1, ("nan", f64::NAN));
        input.insert(2, ("negative", -1.0));
        handle.passes(&input);
        handle.drive();
        let beat = read_apps(&dir).pop().unwrap();
        let passes = beat.gpu_pass_ms.unwrap();
        assert_eq!(passes.len(), 16);
        assert_eq!(passes[&"x".repeat(24)], 2.0);
        assert!(!passes.contains_key("nan"));
        assert!(!passes.contains_key("negative"));
        assert!(passes.contains_key("pass14"));
        assert!(!passes.contains_key("pass15"));
        handle.renderer(&"é".repeat(20));
        handle.passes(&[(&"é".repeat(30), 0.0)]);
        us.store(4_000_000, Ordering::Relaxed);
        handle.drive();
        let beat = read_apps(&dir).pop().unwrap();
        assert_eq!(beat.renderer.unwrap().chars().count(), 16);
        assert_eq!(
            beat.gpu_pass_ms
                .unwrap()
                .keys()
                .next()
                .unwrap()
                .chars()
                .count(),
            24
        );
        drop(handle);
    }

    #[test]
    fn a_wall_clock_step_back_does_not_stop_the_heartbeat() {
        let dir = scratch("live-wall-step");
        let (clock, wall, mono) = Clock::split(10_000_000_000);
        let handle = open(
            "paper",
            "0.12.0",
            Surface::Tui,
            Pacing::OnChange,
            env_at(&dir),
            clock,
            false,
        );
        handle.frame(Duration::from_millis(2));
        handle.drive();
        assert_eq!(read_apps(&dir)[0].updated, 10_000_000);
        wall.store(5_000_000_000, Ordering::Relaxed);
        mono.store(10_000_000_000 + 1_000_000, Ordering::Relaxed);
        handle.frame(Duration::from_millis(2));
        handle.drive();
        let beat = &read_apps(&dir)[0];
        assert_eq!(beat.updated, 5_000_000);
        assert_eq!(beat.fps, 2.0);
        assert_eq!(beat.started, 10_000_000);
        drop(handle);
    }

    #[test]
    fn every_maximum_heartbeat_field_fits_the_file_limit() {
        let dir = scratch("live-limit");
        let (clock, _) = Clock::manual(u64::MAX - 1_000_000);
        let mut cfg = env_at(&dir);
        cfg.ride = true;
        cfg.ride_set = Some("s".repeat(128));
        let handle = open(
            &"a".repeat(128),
            &"v".repeat(128),
            Surface::Desk,
            Pacing::Continuous,
            cfg,
            clock,
            false,
        );
        handle.detail(&"d".repeat(240));
        handle.size(u32::MAX, u32::MAX);
        handle.renderer(&"r".repeat(16));
        let names: Vec<_> = (0..16)
            .map(|i| format!("{i:02}{}", "p".repeat(22)))
            .collect();
        let passes: Vec<_> = names.iter().map(|name| (name.as_str(), f64::MAX)).collect();
        handle.passes(&passes);
        handle.drive();
        let path = dir.join(&handle.inner.as_ref().unwrap().file);
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() <= FILE_LIMIT, "{} bytes", bytes.len());
        let mut beat: Beat = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(beat.detail.as_ref().unwrap().len(), 240);
        assert_eq!(beat.gpu_pass_ms.as_ref().unwrap().len(), 16);
        beat.host = "h".repeat(64);
        beat.id = format!("{}:{}:{}", beat.host, u32::MAX, u64::MAX);
        beat.pid = u32::MAX;
        beat.pid_start = u64::MAX;
        beat.fps = f64::MAX;
        let span = Span {
            p50: f64::MAX,
            p95: f64::MAX,
            p99: f64::MAX,
            max: f64::MAX,
        };
        beat.frame_ms = Some(span.clone());
        beat.gpu_ms = Some(span.clone());
        beat.interval_ms = Some(span);
        beat.missed = u64::MAX;
        beat.target_fps = f64::MAX;
        beat.started = u64::MAX;
        beat.updated = u64::MAX;
        let full = encode(&beat);
        assert!(!full.is_empty());
        assert!(full.len() <= FILE_LIMIT, "{} bytes", full.len());
        let decoded: Beat = serde_json::from_slice(&full).unwrap();
        assert!(decoded.detail.is_some());
        assert_eq!(decoded.gpu_pass_ms.unwrap().len(), 16);
        drop(handle);
    }

    #[test]
    fn old_beat_without_new_fields_still_parses() {
        let old = r#"{"v":1,"app":"studio","frame_ms":{"p50":1.9,"p99":6.8,"max":9.1},"gpu_ms":{"p50":7.8,"p99":8.3,"max":8.9},"interval_ms":{"p50":8.3,"p99":9.4,"max":17.2}}"#;
        let beat: Beat = serde_json::from_str(old).unwrap();
        assert_eq!(beat.frame_ms.unwrap().p95, 0.0);
        assert_eq!(beat.gpu_ms.unwrap().p95, 0.0);
        assert_eq!(beat.interval_ms.unwrap().p95, 0.0);
        assert_eq!(beat.size, None);
        assert_eq!(beat.renderer, None);
        assert_eq!(beat.gpu_pass_ms, None);
    }

    #[test]
    fn an_on_change_surface_misses_nothing_while_it_idles() {
        let dir = scratch("live-on-change");
        let (clock, us) = Clock::manual(10_000_000);
        let handle = open(
            "paper",
            "0.12.0",
            Surface::Tui,
            Pacing::OnChange,
            env_at(&dir),
            clock,
            false,
        );
        for _ in 0..4 {
            us.fetch_add(250_000, Ordering::Relaxed);
            handle.frame(Duration::from_micros(2_000));
        }
        handle.drive();
        let beat = read_apps(&dir).pop().unwrap();
        assert_eq!(beat.pacing, "on_change");
        assert_eq!(beat.fps, 4.0);
        assert_eq!(beat.missed, 0);
        drop(handle);
    }

    #[test]
    fn a_service_writes_without_frames_and_garbage_is_skipped() {
        let dir = scratch("live-service");
        let (clock, _) = Clock::manual(4_000_000);
        let handle = open(
            "dispatch",
            "1.0.0",
            Surface::Service,
            Pacing::OnChange,
            env_at(&dir),
            clock,
            false,
        );
        handle.drive();
        let beat = read_apps(&dir).pop().unwrap();
        assert_eq!(beat.app, "dispatch");
        assert_eq!(beat.surface, "service");
        assert_eq!(beat.fps, 0.0);
        assert!(beat.idle);
        assert!(beat.frame_ms.is_none());
        assert_eq!(beat.target_fps, 0.0);
        assert_eq!(beat.pacing, "on_change");
        std::fs::write(dir.join(".hidden.json"), b"{\"app\":\"nope\"}").unwrap();
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.join("extra.json"), b"{\"extra\":true,\"pid\":3}").unwrap();
        let beats = read_apps(&dir);
        assert_eq!(beats.len(), 1);
        assert_eq!(beats[0].app, "dispatch");
        drop(handle);
    }

    #[test]
    fn the_writer_thread_publishes_and_drop_removes_the_file() {
        let dir = scratch("live-thread");
        let handle = open(
            "paper",
            "0.1.0",
            Surface::Tui,
            Pacing::OnChange,
            env_at(&dir),
            Clock::system(),
            true,
        );
        handle.frame(Duration::from_millis(1));
        let mut appeared = false;
        for _ in 0..40 {
            if !read_apps(&dir).is_empty() {
                appeared = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(appeared, "heartbeat file did not appear");
        drop(handle);
        assert!(read_apps(&dir).is_empty());
    }
}
