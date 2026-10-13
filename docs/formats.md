# pito-jobs' formats

The two files this crate writes and reads back, and the progress lines an op
streams on its stdout. This is their full spec. A change is additive: a new
field is optional, an old reader ignores it, and `v` stays 1.

| § | what | where |
| - | ---- | ----- |
| §1 | a job record | `<jobs>/<pid>-<started>.json` |
| §2 | an app heartbeat | `<jobs>/apps/<pid>-<started>.json` |
| §3 | an op's progress lines | one JSON object per line on the op's stdout |

`<jobs>` is the job folder:

1. `PITO_JOBS_DIR` when it is set;
2. else `$XDG_RUNTIME_DIR/pito-jobs`;
3. else, on macOS only, `$TMPDIR/pito-jobs` (the per-user temporary folder);
4. else there is none, and nothing is written.

`PITO_JOBS=off` turns every write off, so a tool's tests write nothing.

## 1. Job record

A job is one invocation of a tool: one render, one encode, one long run.

**Where:** only in RAM, or the closest thing the OS offers. A tool writes one
file per job in `<jobs>/`, named `<pid>-<started>.json`.
`$XDG_RUNTIME_DIR` is cleared at logout and reboot. Tools never write a
history file; a reader that wants history keeps its own.

The file is rewritten atomically (write a temporary file in the same folder,
then rename), at most once a second, and stays under 4 KiB, with `label` and
`detail` truncated if they need to be:

```json
{"v":1,"id":"desk:1432:1790000000123","host":"desk","pid":1432,"pid_start":98231,
 "tool":"render","app":"trailer","label":"trailer-01",
 "detail":"player @49fe383 1080p→720p web",
 "invoker":"agent","parent":null,"state":"running","stage":"trace",
 "progress":{"done":120,"total":600,"unit":"frames"},"rate":23.4,
 "pgpu":{"pid":1440,"as":"render"},"device":"HIP: AMD Radeon RX 9060 XT",
 "out":"/srv/renders/trailer-01",
 "started":1790000000123,"updated":1790000060123,"ended":null,"exit":null,"error":null}
```

- `id`: host, pid, and start time in ms. Unique across hosts and pid reuse.
- `tool`: the program at work.
- `app`: an optional short key for the app or project being worked on, or
  null.
- `parent`: the id of the job that started this one, or null. The crate hands
  `PITO_JOB_ID` to children through `child_env` and reads it on start, so a
  nested job links itself, and a reader can show child jobs under their
  parent.
- `state`: `running`, `done`, `failed` or `killed`. A tool that knows it's
  waiting may say `waiting`, but it doesn't have to.
- `stage`: optional, a stable identifier so a reader can group and time
  stages across jobs: `build`, `queue`, `record`, `trace`, `replay`,
  `derive`, `master`, `encode`. Counts go in `progress`, not in the name
  (`encode` with `2/5 outputs`, not `encode 2/5`). An optional `stage_note`
  carries the specifics (`dark`, `hevc 1080p@60`). `progress` resets when
  the stage changes, and its `unit` may change with it: `frames` for a clip,
  `samples` for a still, `outputs` for an encode of several files, `seconds`
  for a real-time stage like a terminal recording (no rate there, since it
  runs at 1.0 by construction). `total` is an upper bound: a stage may end
  with done below total, and the stage change or the finish closes it.
- `stages`: optional, the names of the tool's stages in order, each a
  `stage` identifier (`["probe","encode","mux"]`). Absent until the tool
  knows them, which may be after the record started and its first stage
  runs, and it may change, like a plan that grows a `mux`. At most 16 names,
  each capped like `stage`; a record that would pass 4 KiB loses the field
  first. A reader draws a stage bar from it with no list of tool names of
  its own. A record without `stages` draws its current stage alone.
- `label`: what the job is, as a reader shows it, written by the tool as it
  should read (`trailer-01`, `bake courtyard`).
- `rate`: computed by the crate as progress over the last 5 s or the last
  two progress updates, whichever is longer, per second in `progress.unit`.
  A fast unit like samples stays responsive, a slow one like 4K frames
  (seconds each) stays steady, and idle stretches don't spike it. A tool may
  override it. It's absent when meaningless, like a single still.
- `pgpu`: optional, for a tool whose GPU work runs under a GPU queue
  (PITO MD's is pgpu; the field keeps that name). `pgpu.pid` is the pid of the
  queue's process running the tool's current GPU step, and `as` the tool's
  name there; `pgpu` is null while no such process exists, such as during a
  build stage. A tool sets and clears it at any time. A running job with no
  `pgpu` is running on the CPU. A reader may join the pid with the queue's
  own status for waiting and queue position. Inside a queued job a tool has
  no queue child of its own, and the crate defaults the pid from
  `GPU_QUEUE_PID` when the queue sets it.
- `device`: optional, the device the work runs on, like
  `HIP: AMD Radeon RX 9060 XT` or `CPU`. A reader may count anything but
  `CPU` as GPU. It may arrive late, with the first finished render.
- `out`: optional, the absolute path of the result's folder or manifest.
- `error`: optional, one line, truncated, beside a non-zero `exit`.
- `invoker`: optional, who started the tool, from `PITO_INVOKER` when the
  caller sets it.
- `pid_start`: the process's start time (on Linux, field 22 of
  `/proc/self/stat`), so a reused pid is never mistaken for the job.
- `updated`: when the record was last written. The rate is live only while
  this is recent; after a few seconds a reader shows it as stale.
- Times are Unix milliseconds, measured on the host where the job runs.

**Ending:** the tool writes its final state (`done` or `failed`, with
`ended` and `exit`) and leaves the file for a reader, which may archive it
and remove it. A finished file left alone stays until the folder is cleared
at logout or reboot. A reader treats a running record as `killed` when its
pid is gone or its start time no longer matches `pid_start`. Tools often end
with `process::exit` or `_exit`, which run no destructors, so every exit path
must call `finish(code)` or `fail(code, error)` explicitly. Drop is only a
last resort: a job dropped unfinished is written as `failed` with exit code
1.

The crate does all of this for Rust tools, with no background thread of its
own for a job (the heartbeat keeps one, §2) and a small dependency list:
`serde`, `serde_json` and, on unix, `libc`:

```rust
let job = pito_jobs::start("render", Some("trailer"), "trailer-01")
    .detail("player @49fe383 1080p→720p web");
job.stage("build", None, "steps");
job.pgpu(Some(queue_child.id()));
job.stage("trace", Some(600), "frames");
job.progress(120);
job.finish(0);
```

`job.stages(&["probe", "encode", "mux"])` writes the `stages` field now, a
no-op after `finish`, like `stage_note`. A tool may call it once it has
probed and planned, while `probe` already runs:

```rust
let job = pito_jobs::start("encode", None, "clip.mov");
job.stage("probe", None, "files");
job.stages(&["probe", "encode", "mux"]);
job.stage("encode", Some(1), "outputs");
```

`progress` is cheap enough to call every frame, since it writes at most once
a second. The crate is best-effort: a write error or a missing job folder
never fails or slows the tool. `job.stage(…)` starts a new stage and resets
its progress.

## 2. App heartbeat: a running app's own fps

A job ends. A running app (a terminal UI, a desktop window, a service)
doesn't, and its speed is how smoothly it draws. A monitor can measure CPU,
memory, GPU and VRAM from outside, over the app's process tree; only the app
itself knows its fps.

While it runs, the app keeps one file, `<jobs>/apps/<pid>-<started>.json`,
rewritten atomically at most once a second:

```json
{"v":1,"id":"desk:5120:1790000000123","host":"desk","pid":5120,"pid_start":98231,
 "app":"notes","version":"0.12.0","surface":"tui","ride":false,
 "fps":118.6,"frame_ms":{"p50":1.9,"p99":6.8,"max":9.1},"gpu_ms":{"p50":7.8,"p99":8.3,"max":8.9},
 "interval_ms":{"p50":8.3,"p99":9.4,"max":17.2},"missed":1,"target_fps":120,"pacing":"on_change","idle":false,"detail":"converging 128/512 spp","ride_set":null,
 "started":1790000000123,"updated":1790000061123}
```

- `app`: the app's short key. A reader may sum every file with the same `app`
  into one row.
- `surface`: `tui`, `desk` (a GUI window) or `service`. A service is a
  detached daemon that never calls `frame`; it reports only that it's
  alive, so a monitor can add its usage to its app's row.
- `ride`: true when `RIDE_APP` is set and not empty, and `ride_set` carries
  `RIDE_SET`. A test or fixture runner (PITO MD's is prun) sets them before it
  drives the app, and children inherit them, so a reader can tell a test
  run from real use.
- `fps`: frames drawn over the last second. An app that draws only on
  change reports a low fps while idle, and that's correct: `idle` is true
  when nothing was drawn in the last second.
- `frame_ms`: p50, p99 and max of the CPU time spent producing a frame.
- `interval_ms`: p50, p99 and max of the time between presents, which the
  crate measures from its own timestamps. Stutter shows here even when the
  work per frame is small.
- `missed`: intervals longer than 1.5 × the target frame budget, counted only
  on a `continuous` surface. An on-change surface idling between inputs
  misses nothing.
- The percentiles cover the last 5 s, so a 60 fps window has about 300
  samples. `fps` covers the last second.
- `gpu_ms`: optional, p50, p99 and max of GPU time per frame, from the app's
  GPU timestamps. A GPU-bound surface keeps its CPU time tiny while fps
  drops, so this is the number that explains it.
- `detail`: optional, one short line the app chooses, such as
  `converging 128/512 spp` for a progressive tracer.
- Optional and additive:
  - `frame_ms`, `gpu_ms` and `interval_ms` carry `p95` beside `p50`, `p99`
    and `max`, computed from the same samples;
  - `size`: `[w, h]`, the render size in pixels;
  - `renderer`: which renderer drew, such as `live` or `trace`;
  - `gpu_pass_ms`: each GPU pass's rolling mean in ms, such as
    `{"shadow":1.4,"opaque":3.9,"taa":0.9,"post":1.1}`; at most 16 passes,
    names up to 24 characters.
  An older beat without them still reads; a field the app never sets is
  left out.
- `target_fps`: the surface's target: 60 for a desk and 120 for a TUI,
  unless the app says otherwise.
- `pacing`: `continuous` or `on_change`, set by the app. A continuous
  surface is judged by its fps against `target_fps`. An on-change surface
  (a TUI that draws on input and on a 250 ms tick sits near 4 fps) is judged
  by how fast it answers, its `frame_ms` p99, never by fps. `frame_idle()`
  counts a tick-only redraw without holding it against fps.
- Staleness and pid reuse work as in §1 (`updated`, `pid_start`). The crate
  removes the file on a clean exit, and a reader ignores a file whose
  process is gone.

The crate's API is a few calls, cheap enough for every frame:

```rust
let live = pito_jobs::live("notes", env!("CARGO_PKG_VERSION"), pito_jobs::Surface::Tui, pito_jobs::Pacing::OnChange);
live.frame(frame_time);
live.frame_with(cpu_time, Some(gpu_time));
live.detail("converging 128/512 spp");
live.frame_idle();
live.size(3840, 2160);
live.renderer("live");
live.passes(&[("shadow", 1.4), ("opaque", 3.9), ("taa", 0.9), ("post", 1.1)]);
```

`size`, `renderer` and `passes` only store their value; the writer thread
puts the latest into the next once-a-second beat. An app calls them when the
value changes or once a second, never per frame.

Call `frame` once per presented frame: after each draw in a TUI, and on the
redraw path in a window. Headless modes and tests don't call it.

**Opt-in:** the heartbeat runs only where a reader is installed:
`PITO_JOBS_DIR` is set, or the folder `<data>/pito/apps/` exists, where
`<data>` is `$XDG_DATA_HOME`, else `~/.local/share` on Linux and other
unixes and `~/Library/Application Support` on macOS. The monitor's install
creates that folder. Anywhere else, such as an end user's machine running a
shipped app, `live` returns an inert handle, with no thread and no file.

`PITO_JOBS=off` and `PITO_JOBS_DIR` work as in §1. `frame` never allocates
and never locks: it writes a timestamp into a fixed ring. The once-a-second
file write runs on one small low-priority thread that `live` starts, never
on the draw thread. The crate is best-effort and never fails the draw loop.
A `Service` handle writes its file without frame calls.

## 3. Progress lines: an op's `--progress json`

A job record (§1) is a job's state, rewritten in place for whoever looks. A
progress line is the event form of the same thing, streamed to the one
caller that started the op and reads its stdout as it goes: an installer's
`install` or `update`, a `deploy`, a `release`, run with `--progress json`
and read by a TUI's activity band or a script. pci, PITO MD's installer, is
one producer.

With `--progress json`, stdout is one JSON object per line, UTF-8, ended by
`\n` and flushed per line, so a reader sees each event as it happens. The
human lines go to stderr. An op's lines are events, then one closing end
line:

```json
{"v":1,"op":"update","app":"notes","ref":null,"stage":"resolve","state":"start","fraction":null,"msg":"resolving notes","at":1791036610138}
{"v":1,"op":"update","app":"notes","ref":"v0.18.9","stage":"queue","state":"progress","fraction":0.333,"msg":"waiting: 3rd in line","at":1791036640000,"id":"1m411knjtny"}
{"v":1,"op":"update","app":"notes","ref":"v0.18.9","stage":"build","state":"progress","fraction":0.42,"msg":"Compiling 34/80","at":1791036700000,"id":"1m411knjtny"}
{"v":1,"op":"update","app":"notes","ref":"v0.18.9","state":"end","ok":true,"msg":"notes 0.18.9 installed","at":1791036835863,"id":"1m411knjtny"}
```

- `v`: 1. A reader skips a line with any other `v`, or none.
- `op`: what runs, the producer's word: `install`, `update`, `deploy`,
  `release`.
- `app`: the short key of the app the op works on, as in §1. In a batch
  every line carries its own op's `app`.
- `ref`: what the op runs, a tag or a commit, or null until the producer
  knows it.
- `stage`: the current step, a stable identifier as in §1, never carrying a
  count. Stages differ per op and are the producer's; an install might run
  `resolve, clone, verify, queue, build, check, record`. A reader draws a
  stage it has no list for as it reads.
- `state`: `start`, `progress`, `done` or `fail`. A stage starts, may report
  progress, and ends `done` or `fail`; the next stage's `start` may follow at
  once. A reader keeps a line with any other word and treats it as unknown.
- `fraction`: 0 to 1, honest or null: the producer sends null rather than
  guess, and it may fall (a place in a queue falls as the task moves up).
  The writer rounds it to three decimals; a reader clamps one outside 0 to 1.
- `msg`: one short line, or null.
- `at`: when the line was written, Unix milliseconds on the host that runs
  the op.
- `id`: the producer's id for the op, present once there is one; a line
  before that has no `id` field.

The end line closes the op: `{"v":1,"op","app","ref","state":"end","ok",
"msg","at","id"}`. `ok` says whether the op as a whole succeeded, `msg` why
in one line. A producer may add its own fields to the end line (an archive
op's result, what an uninstall left behind); a reader that knows them reads
them and any other ignores them. After its end line an op writes nothing
more.

A batch (several ops after one approval) interleaves its ops' lines, each
op with its own end line, and closes with one summary line:
`{"v":1,"op":"batch","state":"summary","ok","results":[{"op","app","ref",
"ok","msg"}],"msg","at"}`. `ok` is true when every op succeeded, and `msg`
is null, or why the batch ran nothing.

Beside a job record (§1), the same words mean the same things:

| §3 line | §1 record |
| ------- | --------- |
| `op` | `tool`, the program at work |
| `app` | `app` |
| `stage` | `stage` |
| `fraction` | `progress.done` / `progress.total`, null while `total` is unknown |
| `msg` | `detail` and `stage_note` |
| `at` | `updated` |
| `state` `fail`, end `ok` false | `state` `failed`, `error` |
| `id`, the op's | `id`, the job's: each names its own thing, never the other |

The crate reads and writes them in its `op` module. `op::Line::parse` reads
one line into an `Event`, an `End` or a `Summary`, and returns nothing for a
line that is not JSON, has no `v` of 1 or misses what its kind needs (an
event's `stage`); a missing optional field takes its default and an unknown
one is ignored, except that `End::extra` keeps the end line's own fields.
`op::read` reads a stream and skips what does not parse, such as an older
producer's human lines. `op::Writer` writes one op's lines, stamps `v`,
`op`, `app`, `ref`, `id` and `at`, writes each line whole and flushes it,
and writes no event or end after its end line; `Writer::line` writes a line
built by hand, such as an end line with `extra` or a batch's summary.
`Progress::fraction` gives a record's fraction the same way, so one bar
draws both:

```rust
let mut out = pito_jobs::op::Writer::new(std::io::stdout(), "update", "notes");
out.event("resolve", pito_jobs::op::State::Start, None, Some("resolving notes"))?;
out.reference("v0.18.9");
out.id("1m411knjtny");
out.event("build", pito_jobs::op::State::Progress, Some(0.42), Some("Compiling 34/80"))?;
out.end(true, "notes 0.18.9 installed")?;

for line in pito_jobs::op::read(child_stdout) {
    match line {
        pito_jobs::op::Line::Event(event) => draw(&event.app, &event.stage, event.fraction),
        pito_jobs::op::Line::End(end) => finish(&end.app, end.ok, &end.msg),
        pito_jobs::op::Line::Summary(summary) => report(&summary.results),
    }
}
```
