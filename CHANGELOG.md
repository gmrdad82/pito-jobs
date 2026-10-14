# Changelog

## v1.0.0 — 2026-10-14

### Added

- Three new examples: `heartbeat` and `lines` show an app's heartbeat and an op's progress lines read back live, and `bench` measures what each call costs.

### Changed

- Version 1.0.0, with the same API and the same file formats as 0.5.0: nothing to change when you move to it.
- The README shows each feature with its own clip and leads with measured numbers: 26.5 ns to record a frame, 292 ns a progress call.
- The brand reads PITO MD in the README, the notices and the format spec.

## v0.5.0 — 2026-10-12

### Added

- On macOS the job folder falls back to `$TMPDIR/pito-jobs` when `XDG_RUNTIME_DIR` is unset, and the heartbeat's opt-in folder is `~/Library/Application Support/pito/apps`.
- `examples/demo.rs` runs three jobs as child processes and draws their records as a reader would.
- A CI workflow runs the gate on Linux and macOS.

### Changed

- Public under the MIT licence, with docs/formats.md written for any producer and reader.

## v0.4.0 — 2026-10-07

### Added

- The `op` module reads and writes an op's `--progress json` lines (an event, an end and a batch summary), specified as docs/formats.md §3.
- `Progress::fraction` gives a job record's `done / total` the same way, so one bar draws both.

## v0.3.0 — 2026-10-07

### Added

- The job record (§1) and the app heartbeat (§2) in a crate of their own: `start` and `Job`, `live` and `Handle`, `read_dir`, `read_apps` and `apps_dir`.
