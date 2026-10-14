---
title: Link a child job to its parent
description: Pass a job's id to the processes it starts, so their jobs show under it.
sidebar:
  order: 2
---

A tool that starts other tools can link their jobs to its own, so a reader
shows them as children. The link is one environment variable, `PITO_JOB_ID`,
which the child's `start` reads and stores as `parent`.

## In the parent

`child_env` returns the variables a child process needs. Set them on the
command; the crate never changes the current process's environment:

```rust
let job = pito_jobs::start("render", None, "trailer-01");
let mut command = std::process::Command::new("encode");
for (key, value) in job.child_env() {
    command.env(key, value);
}
let status = command.status()?;
```

For the job above, `child_env` returns:

```text
[("PITO_JOB_ID", "desk:223115:1792004530252")]
```

When the parent was started with `PITO_INVOKER` set, the list also carries
`PITO_INVOKER`, so the child records the same invoker.

## In the child

Nothing to do: the child calls `pito_jobs::start` as usual, and its record
carries `"parent":"desk:223115:1792004530252"`. A reader groups records by
`parent` to draw the tree.

## Who started it

`PITO_INVOKER` names who started the tool, such as a person, a script or an
agent, and is stored as `invoker`. Set it in the environment of the first
tool; `child_env` passes it on.
