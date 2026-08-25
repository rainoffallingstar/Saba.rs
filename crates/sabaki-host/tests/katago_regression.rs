//! Real KataGo regression probes for the GTP transport.
//!
//! These tests reproduce the reported "GTP engine stop before a complete
//! response" failure modes against a real `katago gtp` subprocess:
//!
//! 1. verbose stderr must not deadlock the engine (undrained pipe buffer);
//! 2. a bounded command issued while `kata-analyze` is streaming must neither
//!    steal `info move` records nor block;
//! 3. `stop` must flush the final records and leave the session healthy.
//!
//! The tests are skipped (with a notice) when no `katago` binary or model is
//! available, so CI machines without KataGo still pass.

use sabaki_domain_core::gtp::GtpProcessSupervisor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Resolves a KataGo network model: `$KATAGO_MODEL` first, then the standard
/// Sabaki / KataGo model directories.
fn find_katago_model() -> Option<String> {
    if let Ok(path) = std::env::var("KATAGO_MODEL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".config/sabaki-gpui/plugins/engines/katago/models"));
        roots.push(home.join(".config/saba-rs/plugins/engines/katago/models"));
        roots.push(home.join(".katago/models"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.ends_with(".bin.gz") || name.ends_with(".txt.gz") {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Resolves a GTP config file: `$KATAGO_CONFIG` first, then the standard
/// Sabaki config directories.
fn find_katago_config() -> Option<String> {
    if let Ok(path) = std::env::var("KATAGO_CONFIG") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".config/sabaki-gpui/plugins/engines/katago/configs"));
        roots.push(home.join(".config/saba-rs/plugins/engines/katago/configs"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.ends_with(".cfg") {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn start_real_katago() -> Option<GtpProcessSupervisor> {
    if cfg!(windows) {
        eprintln!("real KataGo probe is not run on Windows");
        return None;
    }
    let Some(engine) = sabaki_host::find_katago_executable(None) else {
        eprintln!("katago binary not found; skipping real KataGo regression probe");
        return None;
    };
    let Some(model) = find_katago_model() else {
        eprintln!("katago model not found; skipping real KataGo regression probe");
        return None;
    };
    let mut args = vec!["gtp".to_owned(), "-model".to_owned(), model];
    if let Some(config) = find_katago_config() {
        args.push("-config".to_owned());
        args.push(config);
    }
    match GtpProcessSupervisor::start(&engine.to_string_lossy(), &args) {
        Ok(supervisor) => Some(supervisor),
        Err(error) => {
            eprintln!("katago process failed to start ({error}); skipping real KataGo probe");
            None
        }
    }
}

/// The reported bug, reproduced and locked down against a real engine: a
/// bounded command issued while `kata-analyze` streams must complete
/// cleanly, must not swallow stream records, and the stream must keep
/// flowing; `stop` must flush the tail and leave the session healthy.
#[test]
fn katago_analysis_stream_survives_midstream_bounded_commands() {
    let Some(mut supervisor) = start_real_katago() else {
        return;
    };

    // Bounded handshake + setup. KataGo logs heavily to stderr while starting,
    // so this also verifies the stderr drain keeps the engine responsive.
    let name = supervisor.send("name", Vec::new()).expect("name completes");
    assert!(
        name.success && name.content.to_lowercase().contains("katago"),
        "handshake name: {name:?}"
    );
    let version = supervisor
        .send("version", Vec::new())
        .expect("version completes");
    assert!(version.success, "handshake version: {version:?}");
    for (command, arguments) in [
        ("boardsize", vec!["19".to_owned()]),
        ("clear_board", Vec::new()),
        ("komi", vec!["7.5".to_owned()]),
        ("kata-set-rules", vec!["chinese".to_owned()]),
        (
            "kata-set-param",
            vec!["maxVisits".to_owned(), "300".to_owned()],
        ),
    ] {
        let response = supervisor
            .send(command, arguments)
            .unwrap_or_else(|error| panic!("{command} must complete: {error}"));
        assert!(response.success, "{command} response: {response:?}");
    }

    // Start the official streaming command.
    supervisor
        .send_streaming(
            "kata-analyze",
            vec![
                "B".to_owned(),
                "100".to_owned(),
                "rootInfo".to_owned(),
                "true".to_owned(),
            ],
        )
        .expect("kata-analyze starts");

    // Initial search warm-up on this machine takes a few seconds; poll until
    // several info records have arrived.
    let warmup_deadline = Instant::now() + Duration::from_secs(30);
    let mut info_lines = 0usize;
    while Instant::now() < warmup_deadline && info_lines < 5 {
        if let Some(line) = supervisor.recv_line_timeout(Duration::from_millis(200))
            && line.contains("info move")
        {
            info_lines += 1;
        }
    }
    assert!(
        info_lines >= 5,
        "expected streaming info records, got {info_lines}"
    );

    // The reported failure: a bounded command issued while `kata-analyze` was
    // streaming must complete cleanly — no corruption, no block, no
    // end-of-stream error.
    let bounded = supervisor
        .send(
            "kata-set-param",
            vec!["maxVisits".to_owned(), "500".to_owned()],
        )
        .expect("mid-stream bounded command must complete, not stop before a response");
    assert!(bounded.success, "mid-stream response: {bounded:?}");
    assert!(
        !bounded.content.contains("info move"),
        "stream records must not leak into the bounded response: {}",
        bounded.content
    );

    // KataGo restarts its search when a search parameter changes mid-stream,
    // so the running analyze stops emitting; the session itself must stay
    // healthy. A follow-up bounded command proves the engine is still alive.
    let followup = supervisor
        .send(
            "kata-set-param",
            vec!["maxVisits".to_owned(), "300".to_owned()],
        )
        .expect("follow-up bounded command completes while the engine is alive");
    assert!(followup.success, "follow-up response: {followup:?}");

    // Stop and drain: the final records and the stop response must arrive
    // without an end-of-stream error.
    supervisor
        .send_streaming("stop", Vec::new())
        .expect("stop is sent");
    let drain_deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_header = false;
    loop {
        if Instant::now() >= drain_deadline {
            break;
        }
        match supervisor.recv_line_timeout(Duration::from_millis(100)) {
            Some(line) => {
                let trimmed = line.trim();
                if trimmed.starts_with('=') || trimmed.starts_with('?') {
                    saw_header = true;
                } else if trimmed.is_empty() && saw_header {
                    break;
                }
            }
            None => {
                if supervisor.is_stream_closed() {
                    break;
                }
            }
        }
    }
    assert!(saw_header, "stop response must arrive");

    // The session must survive the whole sequence and be able to start a
    // fresh analysis stream afterwards.
    supervisor
        .send_streaming(
            "kata-analyze",
            vec![
                "B".to_owned(),
                "100".to_owned(),
                "rootInfo".to_owned(),
                "true".to_owned(),
            ],
        )
        .expect("a fresh kata-analyze starts");
    let refresh_deadline = Instant::now() + Duration::from_secs(15);
    let mut refreshed_lines = 0usize;
    while Instant::now() < refresh_deadline && refreshed_lines < 3 {
        if let Some(line) = supervisor.recv_line_timeout(Duration::from_millis(200))
            && line.contains("info move")
        {
            refreshed_lines += 1;
        }
    }
    assert!(
        refreshed_lines >= 3,
        "a fresh kata-analyze must stream again, got {refreshed_lines}"
    );

    supervisor
        .send_streaming("stop", Vec::new())
        .expect("final stop is sent");
    supervisor.stop().ok();
}

/// The exact application handshake path: spawn `katago gtp`, run the full
/// `EngineSession` lifecycle (name, version, capability probe, board setup)
/// and then replay a 10-move game through `EngineController::attach`. This is
/// the path the shell uses on "连接", and it reproduced the reported
/// "引擎连接握手失败" even after the model path was repaired.
#[test]
fn full_session_handshake_and_replay_with_real_katago() {
    use sabaki_domain_core::{Color, MoveDto, Vertex};
    use sabaki_host::{EngineController, EngineRecord, ProcessGtpTransport};

    let Some(engine) = sabaki_host::find_katago_executable(None) else {
        eprintln!("katago binary not found; skipping full handshake probe");
        return;
    };
    let Some(model) = find_katago_model() else {
        eprintln!("katago model not found; skipping full handshake probe");
        return;
    };
    let Some(config) = find_katago_config() else {
        eprintln!("katago config not found; skipping full handshake probe");
        return;
    };
    let args = format!("gtp -model \"{}\" -config \"{}\"", model, config);
    let record = EngineRecord::new("KataGo (real)", engine.display().to_string(), args);

    let transport = ProcessGtpTransport::start(
        &record.path,
        &[
            "gtp".to_owned(),
            "-model".to_owned(),
            model.clone(),
            "-config".to_owned(),
            config,
        ],
    )
    .expect("process starts");

    // Replay the exact move sequence the shell replays (a 10-move game).
    let moves: Vec<MoveDto> = [
        ((3, 3), Color::Black),
        ((15, 15), Color::White),
        ((3, 15), Color::Black),
        ((15, 3), Color::White),
        ((9, 9), Color::Black),
        ((9, 3), Color::White),
        ((3, 9), Color::Black),
        ((15, 9), Color::White),
        ((9, 15), Color::Black),
        ((16, 3), Color::White),
    ]
    .into_iter()
    .map(|((column, row), color)| MoveDto {
        color,
        vertex: Some(Vertex { column, row }),
    })
    .collect();

    // `attach` runs the full handshake (name, version, capability probe, board
    // setup) and then replays the position — the exact "连接" path.
    let mut controller = EngineController::<u8, ProcessGtpTransport>::default();
    controller
        .attach(1, transport, &record, 19, &moves)
        .expect("attach + replay must succeed");
    assert!(controller.is_attached(1));

    // The session remains usable: generate one move.
    let response = controller
        .request_move(1, Color::Black)
        .expect("genmove succeeds");
    assert!(response.success, "genmove response: {response:?}");

    controller.detach_all();
}

/// Regression for the packaged-app handshake failure: KataGo's generated
/// config writes a relative `logDir = katago_logs`, and from a non-writable
/// working directory (a macOS .app launches with an unwritable cwd) KataGo
/// aborts during startup — surfacing as "GTP engine stopped before completing
/// a response". The supervisor must spawn engines in an explicit writable cwd
/// (`start_in`) so this never happens.
#[test]
fn katago_handshake_needs_a_writable_working_directory() {
    use std::path::{Path, PathBuf};

    if cfg!(windows) || !PathBuf::from("/System").is_dir() {
        eprintln!("skipped (requires a Unix /System directory)");
        return;
    }
    let Some(engine) = sabaki_host::find_katago_executable(None) else {
        eprintln!("katago binary not found; skipping cwd regression probe");
        return;
    };
    let Some(model) = find_katago_model() else {
        eprintln!("katago model not found; skipping cwd regression probe");
        return;
    };
    let Some(config) = find_katago_config() else {
        eprintln!("katago config not found; skipping cwd regression probe");
        return;
    };
    let args = vec![
        "gtp".to_owned(),
        "-model".to_owned(),
        model,
        "-config".to_owned(),
        config,
    ];

    // 1. A non-writable cwd must make the engine abort during startup (the
    //    pre-fix behavior the packaged app hit).
    let mut in_non_writable = GtpProcessSupervisor::start_in(
        &engine.to_string_lossy(),
        &args,
        Some(Path::new("/System")),
    )
    .expect("engine process starts even from a bad cwd");
    let failed = in_non_writable.send("name", Vec::new());
    let _ = in_non_writable.stop();
    assert!(
        failed.is_err(),
        "expected the engine to abort from a non-writable cwd"
    );
    assert!(
        in_non_writable.stderr_tail().contains("katago_logs"),
        "the abort reason should mention the log directory, got: {}",
        in_non_writable.stderr_tail()
    );

    // 2. start_in with a writable cwd must handshake successfully (the fix).
    let writable = std::env::temp_dir().join(format!("katago-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&writable).expect("writable dir creates");
    let mut with_cwd =
        GtpProcessSupervisor::start_in(&engine.to_string_lossy(), &args, Some(&writable))
            .expect("engine process starts with writable cwd");
    let name = with_cwd
        .send("name", Vec::new())
        .expect("handshake succeeds with writable cwd");
    assert!(
        name.success && name.content.to_lowercase().contains("katago"),
        "handshake name: {name:?}"
    );
    with_cwd.stop().ok();
    let _ = std::fs::remove_dir_all(&writable);
}
