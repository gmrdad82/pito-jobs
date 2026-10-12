use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use super::env::Settings;
use super::fit::{self, encode};
use super::host::{host_name, pid_start};
use super::rate::Samples;
use super::record::{Pgpu, Progress, Record};
use super::util::{self, Clock, atomic_write};

enum WriteKind {
    Now,
    Progress,
}

struct Inner {
    record: Record,
    dir: Option<PathBuf>,
    off: bool,
    finished: bool,
    last_progress_write: Option<u64>,
    rate_override: Option<f64>,
    samples: Samples,
    clock: Clock,
    file_name: String,
}

#[must_use = "call finish or fail; process::exit does not run drop"]
pub struct Job {
    inner: Mutex<Inner>,
}

pub(crate) fn start_with(
    tool: &str,
    app: Option<&str>,
    label: &str,
    settings: Settings,
    clock: Clock,
) -> Job {
    let now = clock.wall_ms();
    let pid = util::process_id();
    let host = host_name();
    let tool = fit::cap(tool.to_string());
    let id = format!("{host}:{pid}:{now}");
    let file_name = format!("{pid}-{now}.json");
    let pgpu = settings.gpu_queue_pid.map(|pid| Pgpu {
        pid,
        as_name: tool.clone(),
    });
    let record = Record {
        v: 1,
        id,
        host,
        pid,
        pid_start: pid_start(),
        tool,
        app: app.map(|app| fit::cap(app.to_string())),
        label: fit::cap(label.to_string()),
        detail: None,
        invoker: settings.invoker.map(fit::cap),
        parent: settings.parent.map(fit::cap),
        state: "running".to_string(),
        stage: None,
        stages: Vec::new(),
        stage_note: None,
        progress: None,
        rate: None,
        pgpu,
        device: None,
        out: None,
        started: now,
        updated: now,
        ended: None,
        exit: None,
        error: None,
    };
    let job = Job {
        inner: Mutex::new(Inner {
            record,
            dir: settings.dir,
            off: settings.off || !util::WRITES,
            finished: false,
            last_progress_write: None,
            rate_override: None,
            samples: Samples::default(),
            clock,
            file_name,
        }),
    };
    job.commit(WriteKind::Now, now);
    job
}

impl Job {
    pub fn detail(self, detail: impl Into<String>) -> Self {
        self.set_detail(detail.into());
        self
    }

    pub fn device(self, device: impl Into<String>) -> Self {
        self.set_device(device.into());
        self
    }

    pub fn out(self, path: impl AsRef<Path>) -> Self {
        self.set_out(path.as_ref());
        self
    }

    pub fn stage(&self, name: &str, total: Option<u64>, unit: &str) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.stage = Some(fit::cap(name.to_string()));
        inner.record.stage_note = None;
        inner.record.progress = Some(Progress {
            done: 0,
            total,
            unit: fit::cap(unit.to_string()),
        });
        inner.samples.clear();
        inner.last_progress_write = None;
        let rate = inner.shown_rate();
        inner.record.rate = rate;
        inner.commit(WriteKind::Now, now);
    }

    pub fn stages(&self, names: &[&str]) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.stages = names
            .iter()
            .take(fit::MAX_STAGES)
            .map(|name| fit::cap((*name).to_string()))
            .collect();
        inner.commit(WriteKind::Now, now);
    }

    pub fn stage_note(&self, note: impl Into<String>) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.stage_note = Some(fit::cap(note.into()));
        inner.commit(WriteKind::Now, now);
    }

    pub fn progress(&self, done: u64) {
        let mut inner = self.lock();
        if inner.finished || inner.record.progress.is_none() {
            return;
        }
        let now = inner.clock.wall_ms();
        if let Some(progress) = inner.record.progress.as_mut() {
            progress.done = done;
        }
        let mono = inner.clock.mono_ms();
        inner.samples.push(mono, done);
        let rate = inner.shown_rate();
        inner.record.rate = rate;
        inner.commit(WriteKind::Progress, now);
    }

    pub fn pgpu(&self, pid: Option<u32>) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        let tool = inner.record.tool.clone();
        inner.record.pgpu = pid.map(|pid| Pgpu { pid, as_name: tool });
        inner.commit(WriteKind::Now, now);
    }

    pub fn rate(&self, rate: Option<f64>) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.rate_override = rate.filter(|value| value.is_finite());
        let rate = inner.shown_rate();
        inner.record.rate = rate;
        inner.commit(WriteKind::Now, now);
    }

    pub fn finish(&self, code: i32) {
        self.terminal(code == 0, code, None);
    }

    pub fn fail(&self, code: i32, error: impl Into<String>) {
        let code = if code == 0 { 1 } else { code };
        self.terminal(false, code, Some(error.into()));
    }

    pub fn child_env(&self) -> Vec<(&'static str, String)> {
        let inner = self.lock();
        let mut pairs = vec![("PITO_JOB_ID", inner.record.id.clone())];
        if let Some(invoker) = &inner.record.invoker {
            pairs.push(("PITO_INVOKER", invoker.clone()));
        }
        pairs
    }

    fn set_detail(&self, detail: String) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.detail = Some(fit::cap(detail));
        inner.commit(WriteKind::Now, now);
    }

    fn set_device(&self, device: String) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.device = Some(fit::cap(device));
        inner.commit(WriteKind::Now, now);
    }

    fn set_out(&self, path: &Path) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.out = Some(fit::cap(absolute(path)));
        inner.commit(WriteKind::Now, now);
    }

    fn terminal(&self, ok: bool, code: i32, error: Option<String>) {
        let mut inner = self.lock();
        if inner.finished {
            return;
        }
        let now = inner.clock.wall_ms();
        inner.record.state = if ok {
            "done".to_string()
        } else {
            "failed".to_string()
        };
        inner.record.exit = Some(code);
        inner.record.error = error.map(fit::cap);
        inner.record.ended = Some(now);
        inner.finished = true;
        inner.commit(WriteKind::Now, now);
    }

    fn commit(&self, kind: WriteKind, now: u64) {
        self.lock().commit(kind, now);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.fail(1, "dropped");
    }
}

impl Inner {
    fn shown_rate(&self) -> Option<f64> {
        if self
            .record
            .progress
            .as_ref()
            .is_some_and(|progress| progress.unit == "seconds")
        {
            return None;
        }
        if let Some(rate) = self.rate_override.filter(|value| value.is_finite()) {
            return Some(rate);
        }
        self.samples.rate()
    }

    fn commit(&mut self, kind: WriteKind, now: u64) {
        if self.off {
            return;
        }
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let progress = matches!(kind, WriteKind::Progress);
        let mono = self.clock.mono_ms();
        if progress && !progress_due(self.last_progress_write, mono) {
            return;
        }
        let previous_write = self.last_progress_write;
        let previous_updated = self.record.updated;
        if progress {
            self.last_progress_write = Some(mono);
        }
        self.record.updated = now;
        let bytes = encode(&self.record);
        if bytes.is_empty() || atomic_write(&dir, &self.file_name, &bytes).is_err() {
            if progress {
                self.last_progress_write = previous_write;
            }
            self.record.updated = previous_updated;
        }
    }
}

fn progress_due(last: Option<u64>, now: u64) -> bool {
    match last {
        Some(last) if now >= last => now - last >= 1_000,
        _ => true,
    }
}

fn absolute(path: &Path) -> String {
    if path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path).to_string_lossy().into_owned(),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}
