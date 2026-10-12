mod env;
mod fit;
mod host;
mod job;
mod live;
pub mod op;
mod rate;
mod record;
mod util;

pub use job::Job;
pub use live::{Beat, Handle, Pacing, Span, Surface, apps_dir, live, read_apps};
pub use record::{Pgpu, Progress, Record, read_dir};

use env::Settings;
use util::Clock;

pub fn start(tool: impl AsRef<str>, app: Option<&str>, label: impl AsRef<str>) -> Job {
    job::start_with(
        tool.as_ref(),
        app,
        label.as_ref(),
        Settings::from_env(),
        Clock::system(),
    )
}

#[cfg(test)]
mod tests;
