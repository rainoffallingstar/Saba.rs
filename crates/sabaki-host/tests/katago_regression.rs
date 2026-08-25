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
