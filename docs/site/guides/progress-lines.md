---
title: Stream progress lines
description: Print an op's progress as one JSON object per line on stdout, and read those lines from the program that started it.
sidebar:
  order: 5
---

A job record is a job's state, rewritten in place for whoever looks. A
progress line is the event form of the same thing, streamed to the one
caller that started the op and reads its stdout: an installer's `install`, a
`deploy`, a `release`, run with `--progress json`. The `op` module writes
and reads them.

![A terminal: an install op prints its progress lines while a reader draws each one as a bar, with the raw JSON line in grey beneath it: fetch, then build counting Compiling 1/80 to 80/80, then install, ending with a check mark.](../images/lines.gif)

## Write the lines

```rust
use pito_jobs::op::{State, Writer};

let mut out = Writer::new(std::io::stdout(), "install", "notes");
out.event("fetch", State::Start, Some(0.0), Some("v0.18.9"))?;
out.event("build", State::Progress, Some(0.42), Some("Compiling 34/80"))?;
out.end(true, "notes 0.18.9 installed")?;
```

- `event(stage, state, fraction, msg)` writes one line. `state` is `Start`,
  `Progress`, `Done` or `Fail`; `fraction` is 0 to 1, or `None` rather than
  a guess, and is rounded to three decimals.
- `end(ok, msg)` closes the op. The writer writes nothing after it.
- `reference("v0.18.9")` and `id("…")` add what the op runs and the
  producer's id to every later line.
- Each line is written whole and flushed. Keep human output on stderr.

The `lines` example's op prints, among others:

```sh
cargo run --release --example lines emit
```

```json
{"v":1,"op":"install","app":"notes","ref":null,"stage":"fetch","state":"start","fraction":0.0,"msg":"v0.18.9","at":1792004542380}
{"v":1,"op":"install","app":"notes","ref":null,"stage":"fetch","state":"progress","fraction":0.033,"msg":"1/6 MiB","at":1792004542501}
{"v":1,"op":"install","app":"notes","ref":null,"stage":"build","state":"progress","fraction":0.209,"msg":"Compiling 1/80","at":1792004543147}
{"v":1,"state":"end","op":"install","app":"notes","ref":null,"ok":true,"msg":"notes 0.18.9 installed","at":1792004547210}
```

## Read the lines

```rust
use pito_jobs::op::{self, Line};

for line in op::read(child_stdout) {
    match line {
        Line::Event(event) => draw(&event.app, &event.stage, event.fraction),
        Line::End(end) => finish(&end.app, end.ok, &end.msg),
        Line::Summary(summary) => report(&summary.results),
    }
}
```

`op::read` takes any `BufRead`, such as a child's piped stdout in a
`BufReader`. It skips lines that are not JSON or not version 1, such as a
tool's human output, and clamps a fraction to 0 to 1. `Line::parse` reads a
single line. A batch of ops closes with one `Summary` line, and
`Writer::line` writes one built by hand.

To watch an op's lines drawn as bars, run the example without `emit`:

```sh
cargo run --release --example lines
```

The fields and how they map onto a job record are in
[Formats](../../reference/formats/), section 3.
