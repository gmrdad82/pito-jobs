use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use pito_jobs::{Beat, Pacing, Surface};

const APPS: [&str; 2] = ["viewer", "notes"];
const RUN: Duration = Duration::from_secs(7);

fn main() {
    match std::env::var("DEMO_APP") {
        Ok(app) => run(&app),
        Err(_) => watch(),
    }
}

fn run(app: &str) {
    let start = Instant::now();
    let mut seed = 0x9e37_79b9_u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed % 1000) / 1000.0
    };
    if app == "viewer" {
        let live = pito_jobs::live("viewer", "1.4.0", Surface::Desk, Pacing::Continuous);
        live.size(1920, 1080);
        live.renderer("vulkan");
        let budget = Duration::from_micros(16_667);
        let mut frame = 0u64;
        while start.elapsed() < RUN {
            let begin = Instant::now();
            let spike = frame % 97 == 50;
            let cpu = if spike { 21.0 } else { 3.5 + 2.5 * noise() };
            let gpu = if spike { 18.0 } else { 6.0 + 3.0 * noise() };
            sleep(Duration::from_secs_f64(cpu / 1000.0));
            live.frame_with(begin.elapsed(), Some(Duration::from_secs_f64(gpu / 1000.0)));
            live.detail(&format!("orbit, {} instances", 12_000 + (frame % 600) * 10));
            if let Some(rest) = budget.checked_sub(begin.elapsed()) {
                sleep(rest);
            }
            frame += 1;
        }
    } else {
        let live = pito_jobs::live("notes", "0.18.9", Surface::Tui, Pacing::OnChange);
        live.size(104, 30);
        let mut typed = 0;
        while start.elapsed() < RUN {
            let begin = Instant::now();
            if noise() < 0.35 {
                sleep(Duration::from_secs_f64((0.4 + 0.8 * noise()) / 1000.0));
                live.frame(begin.elapsed());
                typed += 1;
                live.detail(&format!("editing todo.md, {typed} keys"));
            } else {
                live.frame_idle();
            }
            sleep(Duration::from_millis(40));
        }
    }
}

fn watch() {
    let dir = std::env::temp_dir().join(format!("pito-jobs-heartbeat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("heartbeat: cannot make {}: {err}", dir.display());
        std::process::exit(1);
    }
    let mut children: Vec<Child> = APPS.iter().filter_map(|app| spawn(app, &dir)).collect();
    let colour = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, text: String| {
        if colour {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text
        }
    };
    let head = format!(
        "  {:<7} {:<8} {:>6}  {:>5} {:>5} {:>5} {:>5}  {:>5}  {:>6}  {}",
        "app", "surface", "fps", "p50", "p95", "p99", "max", "gpu", "missed", "detail"
    );
    if colour {
        print!("\x1b[?25l");
    }
    println!("{}", paint("2", head));
    let mut shown: Vec<Beat> = Vec::new();
    loop {
        for beat in pito_jobs::read_apps(dir.join("apps")) {
            match shown.iter_mut().find(|old| old.app == beat.app) {
                Some(old) => *old = beat,
                None => shown.push(beat),
            }
        }
        shown.sort_by_key(|beat| APPS.iter().position(|app| *app == beat.app));
        let running = children
            .iter_mut()
            .any(|child| matches!(child.try_wait(), Ok(None)));
        let mut out = std::io::stdout().lock();
        for beat in &shown {
            let _ = writeln!(out, "\x1b[2K{}", line(beat, &paint));
        }
        if !running {
            let _ = out.flush();
            break;
        }
        if !shown.is_empty() {
            let _ = write!(out, "\x1b[{}A", shown.len());
        }
        let _ = out.flush();
        sleep(Duration::from_millis(200));
    }
    if colour {
        print!("\x1b[?25h");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn spawn(app: &str, dir: &Path) -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
    Command::new(exe)
        .env("DEMO_APP", app)
        .env("PITO_JOBS_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .ok()
}

fn line(beat: &Beat, paint: &dyn Fn(&str, String) -> String) -> String {
    let ms = |value: f64| format!("{value:>5.1}");
    let (p50, p95, p99, max) = beat.frame_ms.as_ref().map_or_else(
        || {
            (
                "    -".into(),
                "    -".into(),
                "    -".into(),
                "    -".into(),
            )
        },
        |span| (ms(span.p50), ms(span.p95), ms(span.p99), ms(span.max)),
    );
    let gpu = beat
        .gpu_ms
        .as_ref()
        .map_or_else(|| "    -".into(), |span| ms(span.p50));
    let fps = if beat.idle {
        paint("2", format!("{:>6}", "idle"))
    } else {
        paint("32", format!("{:>6.1}", beat.fps))
    };
    let missed = if beat.missed > 0 {
        paint("33", format!("{:>6}", beat.missed))
    } else {
        format!("{:>6}", beat.missed)
    };
    format!(
        "{} {:<7} {:<8} {fps}  {p50} {p95} {} {}  {gpu}  {missed}  {}",
        paint("36", "●".into()),
        beat.app,
        beat.surface,
        p99,
        paint("35", max),
        beat.detail.clone().unwrap_or_default()
    )
}
