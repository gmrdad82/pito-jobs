---
title: Report a job's progress
description: Write a live job record from a command-line tool, with its stages, progress, rate and how it ended.
sidebar:
  order: 1
---

A job is one invocation of a tool: one render, one encode, one long run. The
tool opens it with `start`, moves it through its stages, counts progress and
closes it with `finish` or `fail`. A monitor reads the record as it changes.

![A terminal: three jobs write their records while a reader draws them once a second, an encode and a render finishing with a check mark and a fetch stopping with its error.](../images/demo.gif)

## Start the job

```rust
let job = pito_jobs::start("render", Some("trailer"), "trailer-01")
    .detail("1080p → 720p web")
    .device("GPU")
    .out("/srv/renders/trailer-01");
```

- `tool` (`"render"`): the program at work.
- `app` (`Some("trailer")`): an optional short key for the app or project
  being worked on.
- `label` (`"trailer-01"`): what this run works on, in a few words.
- `detail`, `device` and `out` (the absolute path of the result) are optional
  and chain.

`start` never fails and starts no thread. When there is no job folder, or
`PITO_JOBS=off` is set, it returns a job whose calls do nothing.

## Declare the stages and count progress

```rust
job.stages(&["build", "trace", "encode"]);
job.stage("build", None, "steps");
job.stage("trace", Some(600), "frames");
for frame in 1..=600 {
    job.progress(frame);
}
job.stage_note("hevc 1080p@60");
```

- `stages` lists the stage names in order, so a reader can draw a stage bar
  without knowing the tool. Call it as soon as the plan is known, even after
  the first stage started.
- `stage(name, total, unit)` enters a stage and resets its progress to 0.
  Pass `None` as the total when it is not known yet.
- `progress(done)` sets the count. It updates the rate on every call and
  writes the file at most once a second, so it is safe to call every frame
  or every item (292 ns a call, see [Measure the cost](../measure-the-cost/)).
- `stage_note` carries a stage's specifics. Keep counts out of stage names:
  `encode` with `2/5 outputs`, not `encode 2/5`.
- `rate(Some(value))` overrides the measured rate, and `rate(None)` goes back
  to measuring.

`stage`, `stages`, `stage_note`, `rate`, `finish` and `fail` write at once.

## End it on every path

```rust
match render() {
    Ok(()) => job.finish(0),
    Err(error) => job.fail(2, error.to_string()),
}
```

`finish(code)` ends the job `done` when the code is 0, else `failed`.
`fail(code, error)` ends it `failed` with the error; a code of 0 becomes 1.
`std::process::exit` runs no destructors, so call one of them on every exit
path. Drop is a last resort: a job dropped unfinished is written as `failed`
with exit code 1.

## What the file holds

A record written halfway through an encode, by the crate itself (the host
name changed):

```json
{"v":1,"id":"desk:223115:1792004530252","host":"desk","pid":223115,"pid_start":63451732,"tool":"encode","app":"talk","label":"talk.mp4 → web.mp4","detail":null,"invoker":null,"parent":null,"state":"running","stage":"encode","stages":["probe","encode","mux"],"stage_note":null,"progress":{"done":120,"total":240,"unit":"frames"},"rate":51.55979202772964,"pgpu":null,"device":"GPU","out":null,"started":1792004530252,"updated":1792004532560,"ended":null,"exit":null,"error":null}
```

After `finish(0)` the same file reads `"state":"done"`, with `ended` and
`"exit":0`. Every field is described in [Formats](../../reference/formats/).

The file is rewritten atomically (a temporary file in the same folder, then
a rename) and stays under 4 KiB: when a record would pass it, `stages` is
dropped first, then the longest texts are cut on a character boundary.
