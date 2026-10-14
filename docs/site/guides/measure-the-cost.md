---
title: Measure the cost
description: Run the benchmark that measures what each call costs a tool and a reader.
sidebar:
  order: 7
---

The `bench` example measures what a tool and a reader pay per call, with the
files in the job folder it picks for itself:

```sh
cargo run --release --example bench
```

One run on an AMD Ryzen 9 9950X, Linux, Rust 1.97.1, on a busy machine
(load average about 35), printed:

```text
Job::progress                          235.2 ns
Handle::frame                           26.0 ns
Writer::event, one line                237.3 ns
Line::parse, one line                  777.4 ns
read_dir, per record                  3688.9 ns
job record on disk                       453 B
Job::progress, PITO_JOBS=off           138.3 ns
```

Numbers move from run to run with the machine's load. The chart and table
below are the run of 2026-10-14 on the same machine at a load of about 70,
which the overview quotes:

![Bars, lower is better: the cost per call in nanoseconds of Handle::frame 26.5, Job::progress with PITO_JOBS=off 176, Writer::event 232 and Job::progress 292](../images/cost-bars.png)

| Call | Cost | How |
| --- | --- | --- |
| `Handle::frame` | 26.5 ns | 5,000,000 frames into a live heartbeat |
| `Job::progress` | 292.2 ns | 5,000,000 calls on a live job, its file rewritten once a second |
| `Job::progress`, `PITO_JOBS=off` | 176.0 ns | the same with every write off |
| `Writer::event` | 231.7 ns | 1,000,000 progress lines into `io::sink` |
| `Line::parse` | 670.7 ns | 1,000,000 parses of one progress line |
| `read_dir` | 4.2 µs a record | 2,000 reads of a folder of 32 job records |
| A job record on disk | 454 bytes | the benchmark's own record |

At 60 fps a frame has 16.7 ms, so `Handle::frame` costs 0.00016% of it, and
a `progress` call per frame is under 0.002%.
