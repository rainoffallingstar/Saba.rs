//! Real-subprocess smoke tests for the GTP engine session.
//!
//! These tests spawn `examples/fake-gtp-engine.py` through the production
//! `ProcessGtpTransport`, so the handshake, capability probe, board setup,
//! play/generate and stop paths are exercised over real process pipes. The
//! tests are skipped (with a notice) when the script or a Python interpreter
//! is unavailable, so CI machines without Python still pass.

use ryusei_domain_core::gtp::GtpError;
use ryusei_host::{EngineRecord, EngineSession, EngineSessionState, ProcessGtpTransport};
use std::path::PathBuf;

/// Resolves the fake engine script relative to this crate, or `None` when it
/// is not available.
fn fake_engine_script() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("fake-gtp-engine.py");
    if script.is_file() {
        Some(script.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// Resolves a python3 interpreter, or `None` when unavailable.
fn python3() -> Option<String> {
    let output = std::process::Command::new("python3")
        .arg("--version")
        .output()
        .ok()?;
    output.status.success().then(|| "python3".to_owned())
}

#[test]
fn real_subprocess_session_handshakes_plays_and_stops() {
    let Some(script) = fake_engine_script() else {
        eprintln!("fake engine script not found; skipping real-process smoke test");
        return;
    };
    let Some(python) = python3() else {
        eprintln!("python3 not found; skipping real-process smoke test");
        return;
    };

    let record = EngineRecord::new("FakeGTP", &python, &script);
    let transport = ProcessGtpTransport::start(&record.path, std::slice::from_ref(&record.args))
        .expect("process starts");
    let mut session = EngineSession::start(transport, &record, 9).expect("real session starts");

    assert!(matches!(
        session.state(),
        EngineSessionState::Ready { name, version }
            if name == "FakeGTP" && version == "1.0.0"
    ));
    assert!(session.capabilities().contains("genmove"));
    assert!(session.capabilities().contains("play"));

    let played = session.play("B", "D4").expect("play succeeds");
    assert!(played.success);

    let generated = session.generate_move("W").expect("genmove succeeds");
    assert!(generated.success);
    assert!(!generated.content.is_empty());

    let probe = session
        .send_command("known_command", vec!["genmove".to_owned()])
        .expect("known_command succeeds");
    assert!(probe.success);
    assert_eq!(probe.content, "true");

    session.stop().expect("engine stops");
    assert_eq!(session.state(), &EngineSessionState::Stopped);
}

#[test]
fn a_killed_engine_errors_without_taking_down_the_host() {
    let Some(script) = fake_engine_script() else {
        eprintln!("fake engine script not found; skipping real-process smoke test");
        return;
    };
    let Some(python) = python3() else {
        eprintln!("python3 not found; skipping real-process smoke test");
        return;
    };

    let record = EngineRecord::new("FakeGTP", &python, &script);
    let transport = ProcessGtpTransport::start(&record.path, std::slice::from_ref(&record.args))
        .expect("process starts");
    let mut session = EngineSession::start(transport, &record, 9).expect("real session starts");

    session.stop().expect("engine stops");

    let error = session
        .generate_move("W")
        .expect_err("a dead engine must report an error, not hang the host");

    // The process is gone, so the transport sees an end-of-stream (or a
    // broken pipe) — either way the host keeps running and the session
    // surfaces a typed error instead of panicking.
    assert!(matches!(
        error,
        GtpError::UnexpectedEndOfStream | GtpError::ProcessStart(_)
    ));
}
