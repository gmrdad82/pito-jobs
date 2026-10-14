---
title: Read jobs back
description: List the running and finished job records in the job folder and draw each one's progress.
sidebar:
  order: 3
---

A monitor, a status bar or a script reads the job folder with `read_dir`
and draws what it finds.

## Find the folder

The job folder is, in order:

1. `PITO_JOBS_DIR` when it is set;
2. else `$XDG_RUNTIME_DIR/pito-jobs`;
3. else, on macOS only, `$TMPDIR/pito-jobs`;
4. else there is none, and nothing is written.

A reader resolves it the same way:

```rust
let dir = std::env::var("PITO_JOBS_DIR")
    .or_else(|_| std::env::var("XDG_RUNTIME_DIR").map(|run| format!("{run}/pito-jobs")))
    .unwrap_or_else(|_| "/run/user/1000/pito-jobs".into());
```

## Read the records

```rust
for record in pito_jobs::read_dir(&dir) {
    let percent = record.progress.as_ref().and_then(|p| p.fraction()).map(|f| f * 100.0);
    println!("{} {} {:?} {:?}", record.tool, record.label, record.stage, percent);
}
```

For the encode in [Report a job's progress](../report-a-job/), halfway
through, this prints:

```text
encode talk.mp4 → web.mp4 Some("encode") Some(50.0)
```

- `read_dir` returns the records sorted by file name, which starts with the
  pid. It returns an empty list when the folder does not exist.
- It skips hidden files, folders (such as `apps/`, where heartbeats live), a
  file that does not parse and a file over 4 KiB.
- Unknown fields are ignored, so an old reader reads a newer file.
- `Progress::fraction` is `done / total`, or `None` while the total is
  unknown or 0, clamped to 0 to 1.

## Know what ended

The crate writes `state` as `running`, `done` or `failed`; `ended`, `exit`
and `error` say how a finished job ended. A record stays in the folder after the
job ends, until the folder is cleared (at logout for `$XDG_RUNTIME_DIR`). A
reader that wants a history keeps its own.

A reader treats a `running` record as `killed` when its `pid` is gone or
that process's start time no longer matches `pid_start`, so a reused pid is
never mistaken for the job. `updated` says when the record was last
written: after a few seconds without a write, a reader shows its rate as
stale.
