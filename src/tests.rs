use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Record;
use super::env::Settings;
use super::job::start_with;
use super::read_dir;
use super::util::Clock;

fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tmp")
        .join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn settings(dir: &Path) -> Settings {
    Settings {
        dir: Some(dir.to_path_buf()),
        off: false,
        parent: None,
        invoker: None,
        gpu_queue_pid: None,
    }
}

fn manual(at: u64) -> (Clock, Arc<AtomicU64>) {
    Clock::manual(at * 1000)
}

fn at(ms: &AtomicU64, t: u64) {
    ms.store(t * 1000, Ordering::Relaxed);
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".json") && !name.starts_with('.') {
            files.push(entry.path());
        }
    }
    files.sort();
    files
}

fn only(dir: &Path) -> Record {
    let records = read_dir(dir);
    assert_eq!(records.len(), 1, "{records:?}");
    records.into_iter().next().unwrap()
}

fn near(rate: Option<f64>, expected: f64) {
    let Some(got) = rate else {
        panic!("missing rate, expected {expected}");
    };
    assert!(
        (got - expected).abs() < 1e-6,
        "{got} is not near {expected}"
    );
}

#[test]
fn settings_follow_the_environment_map() {
    let _ = scratch("settings");
    let off = Settings::from_fn(|key| match key {
        "PITO_JOBS" => Some(" off ".to_string()),
        "PITO_JOBS_DIR" => Some("/custom".to_string()),
        "PITO_JOB_ID" => Some(" desk:9:100 ".to_string()),
        "PITO_INVOKER" => Some("agent".to_string()),
        "GPU_QUEUE_PID" => Some("1440".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        _ => None,
    });
    assert!(off.off);
    assert!(off.dir.is_none());
    assert_eq!(off.parent.as_deref(), Some("desk:9:100"));
    assert_eq!(off.invoker.as_deref(), Some("agent"));
    assert_eq!(off.gpu_queue_pid, Some(1440));

    let custom = Settings::from_fn(|key| match key {
        "PITO_JOBS_DIR" => Some("/custom/jobs".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        "GPU_QUEUE_PID" => Some("nope".to_string()),
        "PITO_JOB_ID" => Some("  ".to_string()),
        _ => None,
    });
    assert!(!custom.off);
    assert_eq!(custom.dir, Some(PathBuf::from("/custom/jobs")));
    assert!(custom.parent.is_none());
    assert_eq!(custom.gpu_queue_pid, None);

    let runtime = Settings::from_fn(|key| match key {
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        _ => None,
    });
    assert_eq!(runtime.dir, Some(PathBuf::from("/run/user/1000/pito-jobs")));

    let missing = Settings::from_fn(|_| None);
    assert!(missing.dir.is_none());
    assert!(!missing.off);
}

#[test]
fn macos_falls_back_to_its_own_folders() {
    let mut temp = |key: &str| match key {
        "TMPDIR" => Some("/var/folders/x/T/".to_string()),
        "HOME" => Some("/u".to_string()),
        _ => None,
    };
    assert_eq!(
        crate::env::jobs_dir(&mut temp, true),
        Some(PathBuf::from("/var/folders/x/T/pito-jobs"))
    );
    assert_eq!(crate::env::jobs_dir(&mut temp, false), None);
    assert_eq!(
        crate::env::data_dir(&mut temp, true),
        Some(PathBuf::from("/u/Library/Application Support"))
    );
    assert_eq!(
        crate::env::data_dir(&mut temp, false),
        Some(PathBuf::from("/u/.local/share"))
    );
    let mut xdg = |key: &str| match key {
        "TMPDIR" => Some("/var/folders/x/T".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/501".to_string()),
        "XDG_DATA_HOME" => Some("/data".to_string()),
        _ => None,
    };
    assert_eq!(
        crate::env::jobs_dir(&mut xdg, true),
        Some(PathBuf::from("/run/user/501/pito-jobs"))
    );
    assert_eq!(
        crate::env::data_dir(&mut xdg, true),
        Some(PathBuf::from("/data"))
    );
}

#[test]
fn round_trip_matches_the_record() {
    let dir = scratch("round-trip");
    let started = 1_700_000_000_000;
    let (clock, ms) = manual(started);
    let mut cfg = settings(&dir);
    cfg.parent = Some("desk:9:100".to_string());
    cfg.invoker = Some("agent".to_string());
    cfg.gpu_queue_pid = Some(50);
    let out = dir.join("render");
    let job = start_with("prender-hd", Some("studio"), "trailer-01", cfg, clock)
        .detail("studio player @49fe383 1080p→720p web")
        .device("HIP: AMD Radeon RX 9060 XT")
        .out(&out);

    let running = only(&dir);
    assert_eq!(running.v, 1);
    assert_eq!(running.state, "running");
    assert_eq!(running.pid, std::process::id());
    assert_eq!(
        running.id,
        format!("{}:{}:{}", running.host, running.pid, started)
    );
    assert!(!running.host.is_empty());
    assert!(!running.host.chars().any(|c| c.is_control()));
    assert_eq!(running.started, started);
    assert_eq!(running.tool, "prender-hd");
    assert_eq!(running.app.as_deref(), Some("studio"));
    assert_eq!(running.label, "trailer-01");
    assert_eq!(
        running.detail.as_deref(),
        Some("studio player @49fe383 1080p→720p web")
    );
    assert_eq!(running.parent.as_deref(), Some("desk:9:100"));
    assert_eq!(running.invoker.as_deref(), Some("agent"));
    assert_eq!(
        running.device.as_deref(),
        Some("HIP: AMD Radeon RX 9060 XT")
    );
    assert_eq!(running.out.as_deref(), Some(out.to_str().unwrap()));
    assert!(running.ended.is_none());
    assert!(running.exit.is_none());
    assert!(running.error.is_none());
    assert!(running.stage.is_none());
    let early = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(!early.contains('\n'));
    assert!(early.contains("\"stage_note\":null"));
    assert!(early.contains("\"ended\":null"));
    assert!(early.contains("\"v\":1"));
    assert!(
        json_files(&dir)
            .iter()
            .all(|path| path.file_name().unwrap().to_string_lossy()
                == format!("{}-{started}.json", running.pid))
    );

    let env_before = (
        std::env::var_os("PITO_JOB_ID"),
        std::env::var_os("PITO_INVOKER"),
        std::env::var_os("PITO_JOBS"),
        std::env::var_os("PITO_JOBS_DIR"),
    );
    let env = job.child_env();
    assert_eq!(env.len(), 2);
    assert_eq!(env[0], ("PITO_JOB_ID", running.id.clone()));
    assert_eq!(env[1], ("PITO_INVOKER", "agent".to_string()));
    assert_eq!(
        (
            std::env::var_os("PITO_JOB_ID"),
            std::env::var_os("PITO_INVOKER"),
            std::env::var_os("PITO_JOBS"),
            std::env::var_os("PITO_JOBS_DIR"),
        ),
        env_before
    );

    job.stage("build", None, "steps");
    job.pgpu(Some(1440));
    job.stage("trace", Some(600), "frames");
    job.stage_note("hevc 1080p@60");
    at(&ms, started + 5_000);
    job.progress(0);
    at(&ms, started + 10_000);
    job.progress(120);
    at(&ms, started + 10_500);
    job.finish(0);
    drop(job);

    let rec = only(&dir);
    assert_eq!(rec.state, "done");
    assert_eq!(rec.stage.as_deref(), Some("trace"));
    assert_eq!(rec.stage_note.as_deref(), Some("hevc 1080p@60"));
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 120);
    assert_eq!(progress.total, Some(600));
    assert_eq!(progress.unit, "frames");
    near(rec.rate, 24.0);
    let pgpu = rec.pgpu.unwrap();
    assert_eq!(pgpu.pid, 1440);
    assert_eq!(pgpu.as_name, "prender-hd");
    assert_eq!(rec.ended, Some(started + 10_500));
    assert_eq!(rec.updated, started + 10_500);
    assert_eq!(rec.exit, Some(0));
    assert!(rec.error.is_none());
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|name| !name.starts_with('.')));
}

#[test]
fn rate_uses_the_longer_window() {
    let dir = scratch("rate");
    let (clock, ms) = manual(1_000_000);
    let job = start_with("prender-hd", Some("studio"), "still", settings(&dir), clock);
    job.stage("trace", Some(600), "frames");
    job.progress(0);
    assert_eq!(only(&dir).rate, None, "one sample is a single still");

    at(&ms, 1_001_000);
    job.progress(10);
    near(only(&dir).rate, 10.0);

    at(&ms, 1_006_000);
    job.progress(60);
    near(only(&dir).rate, 10.0);

    job.stage("trace", Some(9), "frames");
    at(&ms, 2_000_000);
    job.progress(0);
    at(&ms, 2_008_000);
    job.progress(1);
    near(only(&dir).rate, 0.125);
    at(&ms, 2_016_000);
    job.progress(2);
    near(only(&dir).rate, 0.125);

    job.stage("trace", Some(1_000), "frames");
    for step in 0..=5 {
        at(&ms, 3_000_000 + step * 1_000);
        job.progress(step * 100);
    }
    near(only(&dir).rate, 100.0);
    at(&ms, 3_020_000);
    job.progress(510);
    near(only(&dir).rate, 10.0 / 15.0);

    job.stage("trace", Some(100), "frames");
    at(&ms, 4_000_000);
    job.progress(0);
    at(&ms, 4_000_500);
    job.progress(50);
    at(&ms, 4_010_000);
    job.progress(50);
    near(only(&dir).rate, 0.0);
    job.finish(0);
}

#[test]
fn seconds_stage_and_override() {
    let dir = scratch("seconds");
    let (clock, ms) = manual(5_000);
    let job = start_with("prender-term", None, "take", settings(&dir), clock);
    assert!(only(&dir).app.is_none());
    job.stage("record", Some(30), "seconds");
    job.progress(0);
    at(&ms, 6_000);
    job.progress(1);
    at(&ms, 7_000);
    job.progress(2);
    assert_eq!(only(&dir).rate, None);
    job.rate(Some(4.0));
    assert_eq!(only(&dir).rate, None, "seconds stays without a rate");

    job.stage("trace", Some(10), "frames");
    assert_eq!(only(&dir).rate, Some(4.0));
    at(&ms, 8_000);
    job.progress(0);
    at(&ms, 9_000);
    job.progress(10);
    assert_eq!(only(&dir).rate, Some(4.0));
    job.rate(None);
    near(only(&dir).rate, 10.0);
    job.rate(Some(f64::NAN));
    near(only(&dir).rate, 10.0);
    job.finish(0);
}

#[test]
fn progress_writes_at_most_once_a_second() {
    let dir = scratch("throttle");
    let (clock, ms) = manual(1_000);
    let job = start_with("prender-hd", Some("studio"), "clip", settings(&dir), clock);
    job.stage("trace", Some(100), "frames");
    at(&ms, 1_100);
    job.progress(7);
    assert_eq!(only(&dir).progress.unwrap().done, 7);

    at(&ms, 1_100 + 999);
    job.progress(8);
    assert_eq!(only(&dir).progress.unwrap().done, 7);
    at(&ms, 1_100 + 1_000);
    job.progress(9);
    assert_eq!(only(&dir).progress.unwrap().done, 9);

    at(&ms, 1_100 + 1_100);
    job.stage("encode", Some(2), "outputs");
    let rec = only(&dir);
    assert_eq!(rec.stage.as_deref(), Some("encode"));
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 0);
    assert_eq!(progress.unit, "outputs");
    assert_eq!(rec.updated, 1_100 + 1_100);

    at(&ms, 1_100 + 1_200);
    job.progress(1);
    assert_eq!(only(&dir).progress.unwrap().done, 1);

    at(&ms, 1_100 + 1_300);
    job.progress(2);
    job.finish(0);
    let rec = only(&dir);
    assert_eq!(rec.progress.unwrap().done, 2);
    assert_eq!(rec.state, "done");
    assert_eq!(rec.updated, 1_100 + 1_300);
}

#[test]
fn stage_resets_progress_and_note() {
    let dir = scratch("stage");
    let (clock, ms) = manual(50);
    let job = start_with("prender-hd", Some("studio"), "clip", settings(&dir), clock);
    job.progress(4);
    assert!(only(&dir).progress.is_none());
    job.stage("trace", Some(10), "frames");
    job.stage_note("dark");
    at(&ms, 1_000);
    job.progress(3);
    let rec = only(&dir);
    assert_eq!(rec.stage_note.as_deref(), Some("dark"));
    assert_eq!(rec.progress.unwrap().done, 3);
    job.stage("encode", Some(2), "outputs");
    let rec = only(&dir);
    assert!(rec.stage_note.is_none());
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 0);
    assert_eq!(progress.total, Some(2));
    assert_eq!(progress.unit, "outputs");
    assert_eq!(rec.rate, None);
    assert_eq!(rec.state, "running");
    job.finish(2);
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(2));
    assert!(rec.error.is_none());
}

#[test]
fn long_text_is_cut_on_a_char_boundary() {
    let dir = scratch("truncate");
    let label = format!("A{}", "😀".repeat(2_000));
    let note = "é".repeat(8_000);
    let error = format!("{}\"\\{}", "n".repeat(5_000), "😀".repeat(1_500));
    let detail = "keep-me-intact";
    let (clock, _) = manual(9);
    let job =
        start_with("prender-hd", Some("studio"), &label, settings(&dir), clock).detail(detail);
    job.stage("trace", Some(1), "frames");
    job.stage_note(&note);
    job.fail(2, &error);
    let files = json_files(&dir);
    let len = std::fs::metadata(&files[0]).unwrap().len();
    assert!(len <= 4096, "{len}");
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(!text.contains('\n'));
    let rec = only(&dir);
    assert_eq!(rec.detail.as_deref(), Some(detail));
    assert!(label.starts_with(&rec.label));
    assert!(rec.label.chars().count() < label.chars().count());
    assert!(!rec.label.contains('\u{FFFD}'));
    let got_note = rec.stage_note.unwrap();
    assert!(note.starts_with(&got_note));
    assert!(got_note.chars().count() < note.chars().count());
    let got_error = rec.error.unwrap();
    assert!(error.starts_with(&got_error));
    assert!(got_error.chars().count() < error.chars().count());
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(2));
}

#[test]
fn off_writes_nothing_and_child_env_still_links() {
    let dir = scratch("off");
    let mut cfg = settings(&dir);
    cfg.off = true;
    cfg.invoker = Some("agent".to_string());
    let (clock, _) = manual(3);
    let job = start_with("prender-hd", Some("studio"), "clip", cfg, clock);
    let env = job.child_env();
    assert_eq!(env.len(), 2);
    assert_eq!(env[0].0, "PITO_JOB_ID");
    assert!(!env[0].1.is_empty());
    assert_eq!(env[1].1, "agent");
    job.stage("trace", Some(3), "frames");
    job.progress(1);
    job.pgpu(Some(9));
    job.finish(0);
    drop(job);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn parent_without_invoker_exports_only_the_job_id() {
    let dir = scratch("parent");
    let mut cfg = settings(&dir);
    cfg.parent = Some("desk:1:2".to_string());
    let (clock, _) = manual(4);
    let job = start_with("prender-assets", None, "logo", cfg, clock);
    let rec = only(&dir);
    assert_eq!(rec.parent.as_deref(), Some("desk:1:2"));
    assert!(rec.invoker.is_none());
    let env = job.child_env();
    assert_eq!(env, vec![("PITO_JOB_ID", rec.id)]);
    job.finish(0);
}

#[test]
fn gpu_queue_pid_is_the_default_and_can_be_cleared() {
    let dir = scratch("pgpu");
    let mut cfg = settings(&dir);
    cfg.gpu_queue_pid = Some(1440);
    let (clock, _) = manual(6);
    let job = start_with("prender-hd", Some("studio"), "clip", cfg, clock);
    let pgpu = only(&dir).pgpu.unwrap();
    assert_eq!(pgpu.pid, 1440);
    assert_eq!(pgpu.as_name, "prender-hd");
    job.pgpu(None);
    assert!(only(&dir).pgpu.is_none());
    job.pgpu(Some(99));
    let pgpu = only(&dir).pgpu.unwrap();
    assert_eq!(pgpu.pid, 99);
    assert_eq!(pgpu.as_name, "prender-hd");
    job.finish(0);
}

#[test]
fn out_paths_are_absolute() {
    let dir = scratch("out");
    let (clock, _) = manual(8);
    let relative = start_with("prender-hd", None, "clip", settings(&dir), clock).out("rel/trailer");
    let expected = std::env::current_dir().unwrap().join("rel/trailer");
    assert_eq!(only(&dir).out.as_deref(), Some(expected.to_str().unwrap()));
    relative.finish(0);

    let dir = scratch("out-abs");
    let abs = dir.join("absolute");
    let (clock, _) = manual(9);
    let job = start_with("prender-hd", None, "clip", settings(&dir), clock).out(&abs);
    assert_eq!(only(&dir).out.as_deref(), Some(abs.to_str().unwrap()));
    job.finish(0);
}

#[test]
fn drop_records_failed_unless_already_finished() {
    let dir = scratch("drop");
    let (clock, ms) = manual(11);
    {
        let job = start_with("prender-hd", Some("studio"), "clip", settings(&dir), clock);
        job.stage("trace", Some(4), "frames");
        at(&ms, 40);
    }
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(1));
    assert_eq!(rec.error.as_deref(), Some("dropped"));
    assert_eq!(rec.ended, Some(40));

    let (clock, _) = manual(12);
    let job = start_with("prender-hd", Some("studio"), "clip", settings(&dir), clock);
    job.finish(0);
    drop(job);
    let records = read_dir(&dir);
    let done = records.iter().find(|rec| rec.started == 12).unwrap();
    assert_eq!(done.state, "done");
    assert_eq!(done.exit, Some(0));
    assert!(done.error.is_none());
}

#[test]
fn fail_wins_and_a_zero_code_becomes_one() {
    let dir = scratch("fail");
    let (clock, _) = manual(13);
    let job = start_with("prender-hd", None, "clip", settings(&dir), clock);
    job.fail(0, "blender died");
    job.finish(0);
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(1));
    assert_eq!(rec.error.as_deref(), Some("blender died"));
    drop(job);
    let rec = only(&dir);
    assert_eq!(rec.error.as_deref(), Some("blender died"));
}

#[test]
fn read_dir_skips_what_it_cannot_parse_and_sorts() {
    let dir = scratch("reader");
    let minimal = |id: &str, pid: u32| {
        format!(
            r#"{{"v":1,"id":"{id}","host":"h","pid":{pid},"pid_start":1,"tool":"t","label":"l","state":"killed","started":1,"updated":2,"extra":true}}"#
        )
    };
    std::fs::write(dir.join("b.json"), minimal("h:2:2", 2)).unwrap();
    std::fs::write(dir.join("a.json"), minimal("h:1:1", 1)).unwrap();
    let mut extra = minimal("h:3:3", 3);
    extra.pop();
    extra.push_str(r#","future":{"n":1}}"#);
    std::fs::write(dir.join("c.json"), extra).unwrap();
    std::fs::write(dir.join("bad.json"), b"{").unwrap();
    std::fs::write(dir.join("note.txt"), b"hello").unwrap();
    std::fs::write(dir.join(".hidden.json"), minimal("h:9:9", 9)).unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(
        dir.join("d.json"),
        br#"{"v":1,"id":"x","host":"h","pid":1,"pid_start":1,"tool":"t","state":"running","started":1,"updated":1}"#,
    )
    .unwrap();

    let records = read_dir(&dir);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].id, "h:1:1");
    assert_eq!(records[1].id, "h:2:2");
    assert_eq!(records[2].id, "h:3:3");
    assert_eq!(records[0].state, "killed");
    assert!(records[0].stage_note.is_none());
    assert!(records[0].app.is_none());
    assert!(read_dir(dir.join("missing")).is_empty());
}

#[test]
fn read_dir_skips_a_file_over_four_kilobytes_and_keeps_one_at_the_cap() {
    let dir = scratch("reader-cap");
    let kept = capped_record(super::util::READ_CAP);
    let mut over = kept.clone();
    over.push(b' ');
    std::fs::write(dir.join("kept.json"), &kept).unwrap();
    std::fs::write(dir.join("over.json"), &over).unwrap();
    let records = read_dir(&dir);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "cap");
    assert_eq!(kept.len(), super::util::READ_CAP);
    assert!(over.len() > super::util::READ_CAP);
}

fn capped_record(len: usize) -> Vec<u8> {
    let prefix = br#"{"v":1,"id":"cap","host":"h","pid":1,"pid_start":1,"tool":"t","label":""#;
    let suffix = br#"","state":"killed","started":1,"updated":2}"#;
    let pad = len - prefix.len() - suffix.len();
    let mut bytes = Vec::with_capacity(len);
    bytes.extend_from_slice(prefix);
    bytes.extend(std::iter::repeat_n(b'a', pad));
    bytes.extend_from_slice(suffix);
    assert_eq!(bytes.len(), len);
    bytes
}

#[test]
fn a_missing_directory_or_a_blocked_path_does_not_panic() {
    let dir = scratch("io");
    let (clock, _) = manual(14);
    let job = start_with(
        "prender-hd",
        None,
        "clip",
        Settings {
            dir: None,
            off: false,
            parent: None,
            invoker: None,
            gpu_queue_pid: None,
        },
        clock,
    );
    job.stage("trace", Some(1), "frames");
    job.progress(1);
    job.finish(0);

    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let (clock, _) = manual(15);
    let job = start_with(
        "prender-hd",
        None,
        "clip",
        Settings {
            dir: Some(blocker.join("jobs")),
            off: false,
            parent: None,
            invoker: None,
            gpu_queue_pid: None,
        },
        clock,
    );
    job.stage("trace", Some(1), "frames");
    job.progress(1);
    job.fail(4, "disk full");
    drop(job);
    assert!(blocker.is_file());
}

#[test]
fn stages_are_written_and_round_trip() {
    let dir = scratch("stages");
    let (clock, _) = manual(70);
    let job = start_with("pcod", Some("convert"), "clip", settings(&dir), clock);
    job.stage("probe", None, "files");
    let early = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(!early.contains("stages"), "{early}");
    assert!(only(&dir).stages.is_empty());

    job.stages(&["probe", "encode"]);
    assert_eq!(only(&dir).stages, ["probe", "encode"]);
    job.stages(&["probe", "encode", "mux"]);
    let rec = only(&dir);
    assert_eq!(rec.stages, ["probe", "encode", "mux"]);
    assert_eq!(rec.stage.as_deref(), Some("probe"));
    let text = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(
        text.contains("\"stages\":[\"probe\",\"encode\",\"mux\"]"),
        "{text}"
    );
    let back: Record = serde_json::from_str(&text).unwrap();
    assert_eq!(back, rec);

    let many: Vec<String> = (0..20).map(|n| format!("s{n}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    job.stages(&names);
    assert_eq!(only(&dir).stages.len(), 16);
    job.finish(0);
}

#[test]
fn stages_are_a_no_op_after_finish() {
    let dir = scratch("stages-finished");
    let (clock, _) = manual(80);
    let job = start_with("pcod", Some("convert"), "clip", settings(&dir), clock);
    job.stages(&["probe", "encode"]);
    job.finish(0);
    job.stages(&["probe", "encode", "mux"]);
    assert_eq!(only(&dir).stages, ["probe", "encode"]);
}

#[test]
fn oversized_stages_are_dropped_to_keep_the_file_small() {
    let dir = scratch("stages-big");
    let (clock, _) = manual(90);
    let job = start_with("pcod", Some("convert"), "clip", settings(&dir), clock);
    let long = "x".repeat(2_000);
    job.stages(&[&long, &long, &long]);
    let len = std::fs::metadata(&json_files(&dir)[0]).unwrap().len();
    assert!(len <= 4096, "{len}");
    assert_eq!(only(&dir).label, "clip");
    job.finish(0);
}

#[test]
fn a_wall_clock_step_back_does_not_loosen_the_progress_throttle_or_the_rate() {
    let dir = scratch("wall-step");
    let (clock, wall, mono) = Clock::split(1_000_000_000);
    let job = start_with("prender-hd", Some("studio"), "clip", settings(&dir), clock);
    job.stage("trace", Some(100), "frames");
    mono.store(1_000_000_000 + 1_000_000, Ordering::Relaxed);
    wall.store(1_000_000_000 + 1_000_000, Ordering::Relaxed);
    job.progress(10);
    assert_eq!(only(&dir).progress.unwrap().done, 10);

    wall.store(500_000_000, Ordering::Relaxed);
    mono.store(1_000_000_000 + 1_100_000, Ordering::Relaxed);
    job.progress(11);
    assert_eq!(only(&dir).progress.unwrap().done, 10);

    mono.store(1_000_000_000 + 2_000_000, Ordering::Relaxed);
    job.progress(20);
    let rec = only(&dir);
    assert_eq!(rec.progress.unwrap().done, 20);
    assert_eq!(rec.updated, 500_000);
    near(rec.rate, 10.0);
    job.finish(0);
}

#[test]
fn the_machine_gives_its_real_pid_here() {
    assert_eq!(super::util::process_id(), std::process::id());
}

#[test]
fn pci_progress_lines_read_as_their_kinds_and_the_rest_is_skipped() {
    use super::op::{self, Line, State};
    let stream = [
        r#"{"app":"pfx","at":1791036610138,"fraction":null,"msg":"resolving","op":"update","ref":null,"stage":"resolve","state":"start","v":1}"#,
        "pci: updating pfx",
        r#"{"app":"pfx","at":1791036700000,"fraction":1.7,"id":"1m411knjtny","msg":"Compiling 34/80","op":"update","ref":"v0.18.9","stage":"build","state":"progress","v":1,"rate":3.5}"#,
        r#"{"app":"pfx","at":1791036700001,"op":"update","stage":"build","state":"skipped","v":1}"#,
        r#"{"app":"pfx","at":1791036700002,"op":"update","stage":"build","state":"done","v":2}"#,
        r#"{"app":"pfx","at":1791036700003,"op":"update","state":"progress","v":1}"#,
        r#"{"app":"docs/demo","archive":{"item":{"id":"x"},"moved":null},"at":1791036835863,"id":"1m4bvr5msrb","msg":"put docs/demo","ok":true,"op":"archive","ref":"put","state":"end","v":1}"#,
        r#"{"at":7,"msg":null,"ok":false,"op":"batch","results":[{"app":"pfx","msg":"pfx 0.18.9 installed","ok":true,"op":"update","ref":"v0.18.9"},{"app":"prec","msg":"lock held","ok":false,"op":"update","ref":null}],"state":"summary","v":1}"#,
    ]
    .join("\n");
    let lines: Vec<Line> = op::read(stream.as_bytes()).collect();
    assert_eq!(lines.len(), 5, "{lines:?}");

    let Line::Event(first) = &lines[0] else {
        panic!("{:?}", lines[0]);
    };
    assert_eq!(
        (first.op.as_str(), first.app.as_str(), first.stage.as_str()),
        ("update", "pfx", "resolve")
    );
    assert_eq!(first.state, State::Start);
    assert_eq!(
        (
            first.reference.as_deref(),
            first.fraction,
            first.id.as_deref()
        ),
        (None, None, None)
    );

    let Line::Event(build) = &lines[1] else {
        panic!("{:?}", lines[1]);
    };
    assert_eq!(build.fraction, Some(1.0));
    assert_eq!(build.id.as_deref(), Some("1m411knjtny"));
    assert_eq!(build.reference.as_deref(), Some("v0.18.9"));

    let Line::Event(odd) = &lines[2] else {
        panic!("{:?}", lines[2]);
    };
    assert_eq!(odd.state, State::Unknown);

    let Line::End(end) = &lines[3] else {
        panic!("{:?}", lines[3]);
    };
    assert!(end.ok);
    assert_eq!(end.msg, "put docs/demo");
    assert_eq!(end.at, 1_791_036_835_863);
    assert_eq!(end.extra.keys().collect::<Vec<_>>(), ["archive"]);
    assert_eq!(end.extra["archive"]["item"]["id"], "x");

    let Line::Summary(summary) = &lines[4] else {
        panic!("{:?}", lines[4]);
    };
    assert!(!summary.ok);
    assert_eq!(summary.results.len(), 2);
    assert_eq!(summary.results[1].app, "prec");
    assert!(!summary.results[1].ok);
    assert_eq!(Line::parse(&lines[3].to_json()).as_ref(), Some(&lines[3]));
    assert_eq!(Line::parse(&lines[4].to_json()).as_ref(), Some(&lines[4]));
}

#[test]
fn the_writer_flushes_one_line_per_call_and_stops_after_the_end() {
    use super::op::{Line, State, Writer};
    use std::io::Write;

    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        flushed_at: Vec<usize>,
    }
    impl Write for &mut Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushed_at.push(self.bytes.len());
            Ok(())
        }
    }

    let mut sink = Sink::default();
    let (clock, ms) = manual(1_791_036_610_000);
    {
        let mut out = Writer::with_clock(&mut sink, "update", "pfx", clock);
        out.event("resolve", State::Start, None, Some("resolving"))
            .unwrap();
        out.reference("v0.18.9");
        out.id("1m411knjtny");
        at(&ms, 1_791_036_700_000);
        out.event(
            "build",
            State::Progress,
            Some(0.123_456),
            Some("Compiling 34/80"),
        )
        .unwrap();
        out.end(true, "pfx 0.18.9 installed").unwrap();
        out.event("build", State::Done, None, None).unwrap();
        out.end(false, "twice").unwrap();
    }

    let text = String::from_utf8(sink.bytes.clone()).unwrap();
    let raw: Vec<&str> = text.lines().collect();
    assert_eq!(raw.len(), 3, "{text}");
    let ends: Vec<usize> = text.match_indices('\n').map(|(at, _)| at + 1).collect();
    assert_eq!(sink.flushed_at, ends);

    let first: serde_json::Value = serde_json::from_str(raw[0]).unwrap();
    assert_eq!(first["v"], 1);
    assert_eq!(first["at"], 1_791_036_610_000u64);
    assert!(first["ref"].is_null() && first["fraction"].is_null());
    assert!(first.get("id").is_none());

    let Some(Line::Event(build)) = Line::parse(raw[1]) else {
        panic!("{}", raw[1]);
    };
    assert_eq!(build.fraction, Some(0.123));
    assert_eq!(build.id.as_deref(), Some("1m411knjtny"));
    assert_eq!(build.reference.as_deref(), Some("v0.18.9"));
    assert_eq!(build.at, 1_791_036_700_000);

    let Some(Line::End(end)) = Line::parse(raw[2]) else {
        panic!("{}", raw[2]);
    };
    assert!(end.ok);
    assert_eq!(end.msg, "pfx 0.18.9 installed");
    assert_eq!(end.id.as_deref(), Some("1m411knjtny"));
    assert!(end.extra.is_empty());
}

#[test]
fn a_record_fraction_lines_up_with_done_over_total() {
    use super::Progress;
    let progress = |done, total| Progress {
        done,
        total,
        unit: "frames".to_string(),
    };
    assert_eq!(progress(120, Some(600)).fraction(), Some(0.2));
    assert_eq!(progress(700, Some(600)).fraction(), Some(1.0));
    assert_eq!(progress(5, Some(0)).fraction(), None);
    assert_eq!(progress(5, None).fraction(), None);
}
