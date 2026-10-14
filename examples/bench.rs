use std::hint::black_box;
use std::process::Command;
use std::time::{Duration, Instant};

use pito_jobs::op::{Line, State, Writer};
use pito_jobs::{Pacing, Surface};

const CALLS: u64 = 5_000_000;

fn main() {
    if std::env::var_os("BENCH_DIR").is_some() {
        measure();
        return;
    }
    let dir = std::env::temp_dir().join(format!("pito-jobs-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let status = Command::new(&exe)
        .env("BENCH_DIR", &dir)
        .env("PITO_JOBS_DIR", &dir)
        .env_remove("PITO_JOBS")
        .status();
    let _ = Command::new(&exe)
        .env("BENCH_DIR", &dir)
        .env("BENCH_OFF", "1")
        .env("PITO_JOBS", "off")
        .status();
    let _ = std::fs::remove_dir_all(&dir);
    if !matches!(status, Ok(status) if status.success()) {
        std::process::exit(1);
    }
}

fn per_call(calls: u64, took: Duration) -> f64 {
    took.as_nanos() as f64 / calls as f64
}

fn report(name: &str, ns: f64) {
    println!("{name:<34} {ns:>9.1} ns");
}

fn measure() {
    let dir = std::path::PathBuf::from(std::env::var_os("BENCH_DIR").unwrap_or_default());
    if std::env::var_os("BENCH_OFF").is_some() {
        let job = pito_jobs::start("bench", None, "off");
        job.stage("count", Some(CALLS), "calls");
        let begin = Instant::now();
        for done in 1..=CALLS {
            job.progress(black_box(done));
        }
        report(
            "Job::progress, PITO_JOBS=off",
            per_call(CALLS, begin.elapsed()),
        );
        job.finish(0);
        return;
    }

    let job = pito_jobs::start("bench", None, "progress").device("CPU");
    job.stages(&["count"]);
    job.stage("count", Some(CALLS), "calls");
    let begin = Instant::now();
    for done in 1..=CALLS {
        job.progress(black_box(done));
    }
    let took = begin.elapsed();
    report("Job::progress", per_call(CALLS, took));
    let size = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .max()
        .unwrap_or(0);
    job.finish(0);

    let live = pito_jobs::live("bench", "0.0.0", Surface::Desk, Pacing::Continuous);
    let work = Duration::from_micros(4_200);
    let begin = Instant::now();
    for _ in 0..CALLS {
        live.frame(black_box(work));
    }
    report("Handle::frame", per_call(CALLS, begin.elapsed()));
    drop(live);

    let lines = 1_000_000;
    let mut out = Writer::new(std::io::sink(), "install", "notes");
    let begin = Instant::now();
    for step in 0..lines {
        let _ = out.event(
            "build",
            State::Progress,
            Some(black_box(step as f64 / lines as f64)),
            Some("Compiling 34/80"),
        );
    }
    report("Writer::event, one line", per_call(lines, begin.elapsed()));

    let mut text = Vec::new();
    let _ = Writer::new(&mut text, "install", "notes").event(
        "build",
        State::Progress,
        Some(0.42),
        Some("Compiling 34/80"),
    );
    let text = String::from_utf8(text).unwrap_or_default();
    let begin = Instant::now();
    for _ in 0..lines {
        black_box(Line::parse(black_box(text.trim_end())));
    }
    report("Line::parse, one line", per_call(lines, begin.elapsed()));

    let jobs: Vec<_> = (0..32)
        .map(|n| {
            let job = pito_jobs::start("render", None, format!("shot-{n:02}")).device("GPU");
            job.stages(&["build", "trace", "encode"]);
            job.stage("trace", Some(600), "frames");
            job.progress(n * 10);
            job
        })
        .collect();
    let rounds = 2_000;
    let begin = Instant::now();
    let mut seen = 0;
    for _ in 0..rounds {
        seen += black_box(pito_jobs::read_dir(&dir)).len();
    }
    report(
        "read_dir, per record",
        per_call(seen as u64, begin.elapsed()),
    );
    for job in &jobs {
        job.finish(0);
    }
    println!("{:<34} {size:>9} B", "job record on disk");
}
