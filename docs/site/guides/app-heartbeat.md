---
title: Publish an app heartbeat
description: Report a running app's fps, frame times, GPU time and missed frames, and read them back.
sidebar:
  order: 4
---

A job ends; a running app (a terminal UI, a desktop window, a service) does
not, and its speed is how smoothly it draws. A monitor can measure CPU,
memory and GPU from outside, but only the app knows its fps. The heartbeat
is the app saying it.

![A terminal: two apps publish their heartbeats while a reader draws them: a desktop viewer at about 59 fps with its p50, p95, p99 and max frame times, its GPU time and 3 missed frames, and a terminal app at 5 fps that idles between keys.](../images/heartbeat.gif)

## Open the heartbeat

```rust
let live = pito_jobs::live(
    "viewer",
    env!("CARGO_PKG_VERSION"),
    pito_jobs::Surface::Desk,
    pito_jobs::Pacing::Continuous,
);
```

- `Surface` is `Tui` (target 120 fps), `Desk`, a GUI window (target 60 fps),
  or `Service`, a daemon that never draws and only says it is alive.
- `Pacing` is `Continuous` for an app that draws every frame, or `OnChange`
  for one that draws only when something changed. Missed frames are counted
  only on a continuous surface.

Keep the handle for the app's life: one low-priority thread rewrites the
file once a second, and dropping the handle removes the file.

## Record each frame

```rust
live.frame(frame_time);
live.frame_with(cpu_time, Some(gpu_time));
live.frame_idle();
live.detail("converging 128/512 spp");
```

- `frame(work)` records the CPU time a frame took; `frame_with` adds its GPU
  time. Each stores a sample in a fixed ring of 2,048 slots (80 KiB,
  allocated once): it never allocates or locks, and costs 26.5 ns.
- `frame_idle` records a tick that drew nothing.
- `detail` sets one short line about what the app is doing; `size(w, h)`,
  `renderer(name)` and `passes(&[("shadow", 0.8), ("main", 3.1)])` add the
  window size, the renderer's name and the GPU time of each pass.

## When it publishes

The heartbeat is opt-in. It publishes only when `PITO_JOBS_DIR` is set or
the folder `pito/apps/` exists in the user's data folder (`$XDG_DATA_HOME`,
else `~/.local/share` on Linux, `~/Library/Application Support` on macOS).
Anywhere else `live` returns an inert handle with no thread and no file, so
an app you ship writes nothing on its users' machines.

## What the file holds

The file is `<jobs>/apps/<pid>-<started>.json`. One written by a desktop
viewer drawing a frame every 16 ms (the host name changed):

```json
{"v":1,"id":"desk:223115:1792004532561","host":"desk","pid":223115,"pid_start":63451732,"app":"viewer","version":"0.3.0","surface":"desk","ride":false,"fps":25.0,"frame_ms":{"p50":4.9,"p95":5.8,"p99":5.8,"max":5.8},"gpu_ms":{"p50":2.5,"p95":2.5,"p99":2.5,"max":2.5},"interval_ms":{"p50":16.1,"p95":16.1,"p99":16.1,"max":16.6},"missed":0,"target_fps":60.0,"pacing":"continuous","idle":false,"detail":null,"ride_set":null,"started":1792004532561,"updated":1792004535564}
```

`frame_ms` is the CPU time per frame and `interval_ms` the time between
presents, each as p50, p95, p99 and max over the last 5 s. Every field is in
[Formats](../../reference/formats/).

## Read heartbeats back

```rust
if let Some(dir) = pito_jobs::apps_dir() {
    for beat in pito_jobs::read_apps(&dir) {
        println!("{} {:.1} {:?}", beat.app, beat.fps, beat.frame_ms);
    }
}
```

```text
viewer 25.0 Some(Span { p50: 4.9, p95: 5.8, p99: 5.8, max: 5.8 })
```

`apps_dir` is the folder `live` publishes to, or `None` when the heartbeat
is off. To watch two apps publish while a reader draws them:

```sh
cargo run --release --example heartbeat
```
