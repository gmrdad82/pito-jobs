---
title: Install
description: Add pito-jobs to a Rust project on Linux, macOS or WSL, and write a first job record.
sidebar:
  order: 1
---

pito-jobs is a Rust library, pulled from GitHub by its tag. It needs Rust
1.91 or newer.

## Linux, macOS and WSL

The same line on all three:

```sh
cargo add pito-jobs --git https://github.com/gmrdad82/pito-jobs --tag {{version}}
```

or in your `Cargo.toml`:

```toml
pito-jobs = { git = "https://github.com/gmrdad82/pito-jobs", tag = "{{version}}" }
```

It works on Linux and macOS, and on Windows through WSL; CI tests it on
Linux and macOS.

## Native Windows and wasm

The crate also builds for native Windows and for `wasm32`. On wasm every
handle is inert, so a cross-platform app can link it unconditionally and
call it everywhere. Native Windows builds, but CI does not run its tests
there.

## Quick start

In the tool:

```rust
let job = pito_jobs::start("encode", None, "talk.mp4 → web.mp4");
job.stage("encode", Some(240), "frames");
for frame in 1..=240 {
    job.progress(frame);
}
job.finish(0);
```

While it runs, the job folder holds one JSON file for it, named
`<pid>-<started>.json`. The folder is `$PITO_JOBS_DIR` when set, else
`$XDG_RUNTIME_DIR/pito-jobs` on Linux, else `$TMPDIR/pito-jobs` on macOS.
Any program can read the records back with `pito_jobs::read_dir`; see
[Read jobs back](../guides/read-jobs/).

To watch three jobs write while a reader draws them, as in the clip on the
[overview](../):

```sh
git clone https://github.com/gmrdad82/pito-jobs && cd pito-jobs
cargo run --release --example demo
```

The other examples are `heartbeat`, `lines` and `bench`, each run the same
way.
