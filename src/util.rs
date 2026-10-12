use std::io::Read;
use std::path::Path;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) const WRITES: bool = !cfg!(target_family = "wasm");
pub(crate) const READ_CAP: usize = 4096;

pub(crate) fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > READ_CAP as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(READ_CAP as u64).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn process_id() -> u32 {
    std::process::id()
}

#[cfg(target_family = "wasm")]
pub(crate) fn process_id() -> u32 {
    0
}

pub(crate) fn filled(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn atomic_write(dir: &Path, file_name: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(file_name);
    let tmp = dir.join(format!(".{file_name}.tmp"));
    if let Err(err) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) enum Clock {
    System(Instant),
    #[cfg(test)]
    Manual {
        wall: Arc<AtomicU64>,
        mono: Arc<AtomicU64>,
    },
}

impl Clock {
    pub(crate) fn system() -> Clock {
        Clock::System(Instant::now())
    }

    #[cfg(test)]
    pub(crate) fn manual(start_us: u64) -> (Clock, Arc<AtomicU64>) {
        let us = Arc::new(AtomicU64::new(start_us));
        let clock = Clock::Manual {
            wall: Arc::clone(&us),
            mono: Arc::clone(&us),
        };
        (clock, us)
    }

    #[cfg(test)]
    pub(crate) fn split(start_us: u64) -> (Clock, Arc<AtomicU64>, Arc<AtomicU64>) {
        let wall = Arc::new(AtomicU64::new(start_us));
        let mono = Arc::new(AtomicU64::new(start_us));
        let clock = Clock::Manual {
            wall: Arc::clone(&wall),
            mono: Arc::clone(&mono),
        };
        (clock, wall, mono)
    }

    pub(crate) fn wall_us(&self) -> u64 {
        match self {
            Clock::System(_) => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX))
                .unwrap_or(0),
            #[cfg(test)]
            Clock::Manual { wall, .. } => wall.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn wall_ms(&self) -> u64 {
        self.wall_us() / 1000
    }

    pub(crate) fn mono_us(&self) -> u64 {
        match self {
            Clock::System(base) => u64::try_from(base.elapsed().as_micros()).unwrap_or(u64::MAX),
            #[cfg(test)]
            Clock::Manual { mono, .. } => mono.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn mono_ms(&self) -> u64 {
        self.mono_us() / 1000
    }
}
