use std::path::PathBuf;

use super::util::filled;

pub(crate) const MACOS: bool = cfg!(target_os = "macos");

#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub dir: Option<PathBuf>,
    pub off: bool,
    pub parent: Option<String>,
    pub invoker: Option<String>,
    pub gpu_queue_pid: Option<u32>,
}

impl Settings {
    pub(crate) fn from_env() -> Settings {
        Self::from_fn(|key| std::env::var(key).ok())
    }

    pub(crate) fn from_fn(mut get: impl FnMut(&str) -> Option<String>) -> Settings {
        let off = get("PITO_JOBS")
            .as_deref()
            .is_some_and(|v| v.trim() == "off");
        let parent = filled(get("PITO_JOB_ID"));
        let invoker = filled(get("PITO_INVOKER"));
        let gpu_queue_pid = get("GPU_QUEUE_PID").and_then(|raw| raw.trim().parse().ok());
        let dir = if off { None } else { jobs_dir(&mut get, MACOS) };
        Settings {
            dir,
            off,
            parent,
            invoker,
            gpu_queue_pid,
        }
    }
}

pub(crate) fn jobs_dir(
    get: &mut impl FnMut(&str) -> Option<String>,
    macos: bool,
) -> Option<PathBuf> {
    if let Some(dir) = filled(get("PITO_JOBS_DIR")) {
        return Some(PathBuf::from(dir));
    }
    let runtime = match filled(get("XDG_RUNTIME_DIR")) {
        Some(dir) => dir,
        None if macos => filled(get("TMPDIR"))?,
        None => return None,
    };
    Some(PathBuf::from(runtime).join("pito-jobs"))
}

pub(crate) fn data_dir(
    get: &mut impl FnMut(&str) -> Option<String>,
    macos: bool,
) -> Option<PathBuf> {
    if let Some(data) = filled(get("XDG_DATA_HOME")) {
        return Some(PathBuf::from(data));
    }
    let home = PathBuf::from(filled(get("HOME"))?);
    Some(if macos {
        home.join("Library").join("Application Support")
    } else {
        home.join(".local").join("share")
    })
}
