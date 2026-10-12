use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use pito_jobs::Record;

const JOBS: [&str; 3] = ["encode", "render", "fetch"];
const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn main() {
    match std::env::var("DEMO_JOB") {
        Ok(kind) => work(&kind),
        Err(_) => watch(),
    }
}

fn tick(ms: u64) {
    sleep(Duration::from_millis(ms));
}

fn work(kind: &str) {
    match kind {
        "encode" => {
            let job = pito_jobs::start("encode", None, "talk.mp4 → web.mp4");
            job.stage("probe", Some(1), "files");
            job.stages(&["probe", "encode", "mux"]);
            tick(500);
            job.progress(1);
            job.stage("encode", Some(240), "frames");
            for frame in 1..=240 {
                tick(22);
                job.progress(frame);
            }
            job.stage("mux", Some(1), "files");
            tick(400);
            job.progress(1);
            job.finish(0);
        }
        "render" => {
            let job = pito_jobs::start("render", None, "trailer-01").device("GPU");
            job.stages(&["build", "trace", "encode"]);
            job.stage("build", None, "steps");
            tick(1200);
            job.stage("trace", Some(96), "frames");
            for frame in 1..=96 {
                tick(45);
                job.progress(frame);
            }
            job.stage("encode", Some(1), "outputs");
            tick(500);
            job.progress(1);
            job.finish(0);
        }
        _ => {
            let job = pito_jobs::start("fetch", None, "assets.tar");
            job.stages(&["download", "verify"]);
            job.stage("download", Some(512), "MiB");
            for mib in (8..=296).step_by(8) {
                tick(110);
                job.progress(mib);
            }
            job.fail(1, "connection reset by peer");
        }
    }
}

fn watch() {
    let dir = std::env::temp_dir().join(format!("pito-jobs-demo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("demo: cannot make {}: {err}", dir.display());
        std::process::exit(1);
    }
    let mut children: Vec<Child> = JOBS.iter().filter_map(|kind| spawn(kind, &dir)).collect();
    let colour = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut drawn = 0;
    let mut frame = 0;
    loop {
        let mut records = pito_jobs::read_dir(&dir);
        records.sort_by_key(|record| JOBS.iter().position(|kind| *kind == record.tool));
        let running = children
            .iter_mut()
            .any(|child| matches!(child.try_wait(), Ok(None)));
        let mut out = std::io::stdout().lock();
        if drawn > 0 {
            let _ = write!(out, "\x1b[{drawn}A");
        }
        for record in &records {
            let _ = writeln!(out, "\x1b[2K{}", line(record, frame, colour));
        }
        let _ = out.flush();
        drawn = records.len();
        if !running {
            break;
        }
        frame += 1;
        tick(100);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn spawn(kind: &str, dir: &Path) -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
    Command::new(exe)
        .env("DEMO_JOB", kind)
        .env("PITO_JOBS_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .ok()
}

fn line(record: &Record, frame: usize, colour: bool) -> String {
    let paint = |code: &str, text: &str| {
        if colour {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    let mark = match record.state.as_str() {
        "done" => paint("32", "✓"),
        "failed" => paint("31", "✗"),
        _ => paint("36", &SPIN[frame % SPIN.len()].to_string()),
    };
    let stage = record.stage.clone().unwrap_or_default();
    let place = record
        .stages
        .iter()
        .position(|name| *name == stage)
        .map(|at| format!(" {}/{}", at + 1, record.stages.len()))
        .unwrap_or_default();
    let head = format!(
        "{mark} {:<7} {:<19} {:<12}",
        record.tool,
        record.label,
        format!("{stage}{place}")
    );
    if let Some(error) = &record.error {
        return format!("{head} {}", paint("31", error));
    }
    let Some(progress) = &record.progress else {
        return head;
    };
    let fraction = progress.fraction();
    let bar = bar(fraction.unwrap_or(0.0), 16);
    let percent = fraction.map_or("    ".to_string(), |f| format!("{:>3.0}%", f * 100.0));
    let count = match progress.total {
        Some(total) => format!("{}/{total} {}", progress.done, progress.unit),
        None => format!("{} {}", progress.done, progress.unit),
    };
    let rate = record
        .rate
        .filter(|_| record.state == "running")
        .map(|rate| format!("  {rate:.0} {}/s", progress.unit))
        .unwrap_or_default();
    format!("{head} {} {percent}  {count}{rate}", paint("34", &bar))
}

fn bar(fraction: f64, width: usize) -> String {
    let full = (fraction.clamp(0.0, 1.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(full), "░".repeat(width - full))
}
