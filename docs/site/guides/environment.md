---
title: Configure with environment variables
description: Point the files at a folder, turn every write off in tests, and pass a job's context to its children.
sidebar:
  order: 6
---

The crate has no configuration file and no setup call: it reads a few
environment variables when a job or a heartbeat starts. It never changes the
current process's environment.

| Variable | Effect |
| --- | --- |
| `PITO_JOBS_DIR` | Folder for the files. When unset, `$XDG_RUNTIME_DIR/pito-jobs`, or `$TMPDIR/pito-jobs` on macOS. |
| `PITO_JOBS=off` | Every write is a no-op, such as in a tool's own tests. |
| `PITO_JOB_ID` | Id of the job that started this one, stored as `parent`. |
| `PITO_INVOKER` | Who started the tool, stored as `invoker` and passed on to children. |
| `GPU_QUEUE_PID` | Pid of a surrounding GPU queue run, stored as `pgpu.pid` until the tool sets or clears it. |
| `RIDE_APP`, `RIDE_SET` | Set by a test or fixture runner; a heartbeat records them as `ride` and `ride_set`. |

## Keep a tool's tests from writing

```sh
PITO_JOBS=off cargo test
```

Every job and every heartbeat is inert: nothing is written and no folder is
created, and the calls still cost next to nothing (see
[Measure the cost](../measure-the-cost/)).

## Write somewhere you can see

In the project of a tool that links the crate:

```sh
mkdir -p tmp/jobs
PITO_JOBS_DIR=$PWD/tmp/jobs cargo run --release
cat tmp/jobs/*.json
```

The crate's own examples (`demo`, `heartbeat`, `bench`) pick a temporary
folder of their own and set `PITO_JOBS_DIR` for the jobs they start.

`PITO_JOBS_DIR` also turns the heartbeat on: with it set, `live` publishes
to `$PITO_JOBS_DIR/apps/` (see [Publish an app heartbeat](../app-heartbeat/)).

## Pass context to children

`PITO_JOB_ID` and `PITO_INVOKER` are what `Job::child_env` returns; set them
on a child's command and its job links itself (see
[Link a child job to its parent](../child-jobs/)).

`GPU_QUEUE_PID` is set by a GPU queue around the job it runs; the job
records it as `pgpu.pid` until the tool calls `job.pgpu(…)` itself.
