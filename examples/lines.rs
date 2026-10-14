use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use pito_jobs::op::{Line, State, Writer};

const WIDTH: usize = 100;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("emit") {
        let _ = emit();
    } else {
        watch();
    }
}

fn tick(ms: u64) {
    sleep(Duration::from_millis(ms));
}

fn emit() -> std::io::Result<()> {
    let mut out = Writer::new(std::io::stdout(), "install", "notes");
    out.event("fetch", State::Start, Some(0.0), Some("v0.18.9"))?;
    for mib in 1..=6 {
        tick(120);
        let fraction = f64::from(mib) / 6.0 * 0.2;
        out.event(
            "fetch",
            State::Progress,
            Some(fraction),
            Some(&format!("{mib}/6 MiB")),
        )?;
    }
    for crate_ in 1..=80 {
        tick(45);
        let fraction = 0.2 + f64::from(crate_) / 80.0 * 0.7;
        out.event(
            "build",
            State::Progress,
            Some(fraction),
            Some(&format!("Compiling {crate_}/80")),
        )?;
    }
    tick(300);
    out.event(
        "install",
        State::Done,
        Some(1.0),
        Some("~/.local/bin/notes"),
    )?;
    tick(200);
    out.end(true, "notes 0.18.9 installed")
}

fn watch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(mut child) = Command::new(exe)
        .arg("emit")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
    else {
        eprintln!("lines: cannot start the op");
        std::process::exit(1);
    };
    let Some(stdout) = child.stdout.take() else {
        return;
    };
    let colour = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |code: &str, text: &str| {
        if colour {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    let mut drawn = false;
    for text in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Some(line) = Line::parse(&text) else {
            continue;
        };
        let top = match &line {
            Line::Event(event) => {
                let fraction = event.fraction.unwrap_or(0.0);
                format!(
                    "{} {:<6} {:<8} {} {:>3.0}%  {}",
                    paint("36", "●"),
                    event.app,
                    event.stage,
                    paint("34", &bar(fraction, 24)),
                    fraction * 100.0,
                    event.msg.clone().unwrap_or_default()
                )
            }
            Line::End(end) if end.ok => format!("{} {}", paint("32", "✓"), end.msg),
            Line::End(end) => format!("{} {}", paint("31", "✗"), end.msg),
            Line::Summary(summary) => format!("{} results", summary.results.len()),
        };
        let raw: String = if text.chars().count() > WIDTH {
            text.chars().take(WIDTH - 1).chain(['…']).collect()
        } else {
            text.clone()
        };
        let mut out = std::io::stdout().lock();
        if drawn {
            let _ = write!(out, "\x1b[2A");
        }
        let _ = writeln!(out, "\x1b[2K{top}");
        let _ = writeln!(out, "\x1b[2K{}", paint("2", &raw));
        let _ = out.flush();
        drawn = true;
    }
    let _ = child.wait();
}

fn bar(fraction: f64, width: usize) -> String {
    let full = (fraction.clamp(0.0, 1.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(full), "░".repeat(width - full))
}
