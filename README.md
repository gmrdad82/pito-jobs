<p align="center"><img src="docs/images/demo.gif" width="800" alt="A terminal: the demo example starts three jobs that write their records while a reader draws them once a second: an encode moves through probe, encode and mux and a render through build, trace and encode, each with a bar, a count and a rate, until both finish with a check mark, and a fetch stops halfway with its error, connection reset by peer."></p>

# pito-jobs

[![CI](https://github.com/gmrdad82/pito-jobs/actions/workflows/ci.yml/badge.svg)](https://github.com/gmrdad82/pito-jobs/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/tag/gmrdad82/pito-jobs)](https://github.com/gmrdad82/pito-jobs/tags)

pito-jobs is a small Rust crate that lets a command-line tool or an app say
what it is doing, so another program can show it live. It is part of
[PITO](https://pitomd.com), where every tool writes through it and a
monitor reads it back.

It has three forms, all JSON and all best-effort: a write error never fails,
blocks or slows the tool.

- **A job record:** one file per running job, with its stage, progress, rate,
  device, output and how it ended, rewritten atomically at most once a second
  and kept under 4 KiB.
- **An app heartbeat:** one file per running app with its fps, frame times
  (p50, p95, p99, max), GPU time and missed frames, written by one
  low-priority thread so the draw loop never waits.
- **Progress lines:** the `--progress json` lines an op prints on stdout, one
  object per line, flushed per line, with a tolerant reader and a writer.

The files live in RAM: `$XDG_RUNTIME_DIR/pito-jobs/` on Linux,
`$TMPDIR/pito-jobs/` on macOS, or wherever `PITO_JOBS_DIR` points.
[docs/formats.md](docs/formats.md) is the full spec of all three.

Works on Linux and macOS (Windows through WSL), CI-tested on both. It also
builds for native Windows and for `wasm32`, where every handle is inert, so a
cross-platform app can link it unconditionally.

## Install

The same line on Linux, macOS and WSL, in your `Cargo.toml`:

```toml
pito-jobs = { git = "https://github.com/gmrdad82/pito-jobs", tag = "v0.5.0" }
```

It needs Rust 1.91 or newer and brings `serde`, `serde_json` and, on unix,
`libc`.

Run the demo to see a reader draw three jobs as they write (the GIF above):

```sh
cargo run --release --example demo
```

## Use

### A job

```rust
let job = pito_jobs::start("render", None, "trailer-01")
    .detail("1080p → 720p web")
    .device("GPU");
job.stages(&["build", "trace", "encode"]);
job.stage("build", None, "steps");
job.stage("trace", Some(600), "frames");
for frame in 1..=600 {
    job.progress(frame);
}
job.finish(0);
```

`progress` is safe to call every frame: it updates the rate on every call and
writes the file at most once a second. `stage`, `stages`, `stage_note`,
`rate`, `finish` and `fail` write at once. `stages` declares the stage names
in order, so a reader can draw a stage bar without knowing the tool; call it
as soon as the plan is known, even after the first stage started.

`process::exit` runs no destructors, so call `finish(code)` or
`fail(code, error)` on every exit path. Drop is a last resort: a job dropped
unfinished is written as `failed` with exit code 1.

`child_env` returns the variables a child process needs to link its own job
as a child of this one. The crate never changes the current process's
environment:

```rust
for (key, value) in job.child_env() {
    command.env(key, value);
}
```

When a record would pass 4 KiB, `stages` is dropped first, then the longest
texts are cut on a character boundary.

### Reading jobs back

```rust
let dir = std::env::var("PITO_JOBS_DIR").unwrap_or_else(|_| "/run/user/1000/pito-jobs".into());
for record in pito_jobs::read_dir(dir) {
    let percent = record.progress.as_ref().and_then(|p| p.fraction()).map(|f| f * 100.0);
    println!("{} {} {:?} {:?}", record.tool, record.label, record.stage, percent);
}
```

`read_dir` skips a file that does not parse, and unknown fields are ignored,
so an old reader reads a newer file.

### An app's heartbeat

```rust
let live = pito_jobs::live(
    "notes",
    env!("CARGO_PKG_VERSION"),
    pito_jobs::Surface::Tui,
    pito_jobs::Pacing::OnChange,
);
live.frame(frame_time);
live.frame_with(cpu_time, Some(gpu_time));
live.detail("converging 128/512 spp");
```

`frame` stores a sample in a fixed ring: it never allocates or locks. One
low-priority thread rewrites the file once a second and removes it when the
handle is dropped. The heartbeat is opt-in: it publishes only when
`PITO_JOBS_DIR` is set or the folder `pito/apps/` exists in the user's data
folder (`$XDG_DATA_HOME`, else `~/.local/share` on Linux and
`~/Library/Application Support` on macOS). Anywhere else `live` returns an
inert handle with no thread and no file, so an app you ship writes nothing
on its users' machines. `apps_dir` and `read_apps` read the heartbeats back.

### Progress lines

```rust
use pito_jobs::op::{self, Line, State, Writer};

let mut out = Writer::new(std::io::stdout(), "install", "notes");
out.event("build", State::Progress, Some(0.42), Some("Compiling 34/80"))?;
out.end(true, "notes 0.18.9 installed")?;

for line in op::read(child_stdout) {
    match line {
        Line::Event(event) => draw(&event.app, &event.stage, event.fraction),
        Line::End(end) => finish(&end.app, end.ok, &end.msg),
        Line::Summary(summary) => report(&summary.results),
    }
}
```

`Writer` stamps the version, op, app and time on every line, writes each line
whole and flushes it, and writes nothing after the end line. `op::read`
skips lines that are not JSON or not version 1, such as a tool's human
output, and clamps a fraction to 0 to 1.

### Environment

| Variable | Effect |
| --- | --- |
| `PITO_JOBS_DIR` | Folder for the files. When unset, `$XDG_RUNTIME_DIR/pito-jobs`, or `$TMPDIR/pito-jobs` on macOS. |
| `PITO_JOBS=off` | Every write is a no-op, such as in a tool's own tests. |
| `PITO_JOB_ID` | Id of the job that started this one, stored as `parent`. |
| `PITO_INVOKER` | Who started the tool, stored as `invoker` and passed on to children. |
| `GPU_QUEUE_PID` | Pid of a surrounding GPU queue run, stored as `pgpu.pid` until the tool sets or clears it. |
| `RIDE_APP`, `RIDE_SET` | Set by a test or fixture runner; a heartbeat records them as `ride` and `ride_set`. |

## Build and test

```sh
bin/gate --fast   # rustfmt, clippy with -D warnings, the tests, a wasm32-wasip2 check
bin/gate          # adds the doc tests, Windows and macOS checks and the demo's release build
```

The tests use [cargo-nextest](https://nexte.st). The gate needs the
`wasm32-wasip2`, `x86_64-pc-windows-gnu` and `aarch64-apple-darwin` targets
(`rustup target add …`). CI runs `bin/gate` on Linux and macOS.

## Contributing

Issues and pull requests are welcome. The files and lines are a contract that
other programs read, so every change is additive: a new field is optional, an
old reader still reads a new file and a new reader an old one, `v` stays 1,
and a file stays under 4 KiB. A change to a format updates
[docs/formats.md](docs/formats.md) and this README in the same pull request.
`bin/gate` must pass. Please follow the [code of conduct](CODE_OF_CONDUCT.md),
and report security issues privately as [SECURITY.md](SECURITY.md) says.

## Licence

MIT, © Catalin Ilinca: see [LICENSE](LICENSE). The licence covers the code
only: the PITO name, every app and game name and their logos are © Catalin
Ilinca, all rights reserved ([TRADEMARKS.md](TRADEMARKS.md)).
