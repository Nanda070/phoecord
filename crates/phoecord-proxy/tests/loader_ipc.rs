//! The loader ↔ proxy pipe on Windows: the proxy-side server and a fake loader client.
#![cfg(windows)]

use phoecord_loader::{
    ipc::{self, Msg},
    pipe,
};
use phoecord_proxy::ipc::{Sink, spawn_on};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn pipe_name(tag: &str) -> String {
    format!(r"\\.\pipe\phoecord-test-{tag}-{}", std::process::id())
}

fn config(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("phoecord-test-pipe-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("config.toml")
}

fn start(tag: &str) -> (String, mpsc::Receiver<String>, std::path::PathBuf) {
    let name = pipe_name(tag);
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let sink: Sink = Arc::new(move |l| {
        let _ = tx.lock().unwrap().send(l);
    });
    let cfg = config(tag);
    spawn_on(&name, cfg.clone(), sink);
    (name, rx, cfg)
}

#[test]
fn a_fake_loader_says_hello_and_the_proxy_logs_it() {
    let (name, rx, _) = start("hello");
    pipe::send_all(
        &name,
        &[
            Msg::Hello {
                loader_version: "0.5.0".into(),
                client_version: Some("1.0.1355".into()),
                text_hash: "deadbeef".into(),
                pid: std::process::id(),
                detect_ms: 3,
            },
            Msg::Status {
                ui: false,
                reason: Some("no signatures".into()),
            },
        ],
    )
    .expect("the pipe is there");
    let line = rx.recv_timeout(Duration::from_secs(5)).expect("a log line");
    assert!(
        line.starts_with(
            "loader connected: build 1.0.1355, hash deadbeef, UI layer off (no signatures)"
        ),
        "{line}"
    );
}

#[test]
fn settings_travel_over_the_pipe_and_land_in_config_toml() {
    let (name, _rx, cfg) = start("settings");
    let f = pipe::connect(&name).unwrap();
    let mut w = &f;
    let mut r = BufReader::new(&f);
    w.write_all(
        &ipc::encode_line(&Msg::SettingsSet {
            id: 1,
            key: "ui".into(),
            value: serde_json::json!(true),
        })
        .unwrap(),
    )
    .unwrap();
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    assert!(
        matches!(
            ipc::decode_line(line.as_bytes()),
            Ok(Msg::SettingsValue {
                id: 1,
                error: None,
                ..
            })
        ),
        "{line}"
    );
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("ui = true"));
    // Garbage drops the connection: the next read is the end of the stream.
    w.write_all(b"garbage\n").unwrap();
    line.clear();
    assert_eq!(r.read_line(&mut line).unwrap_or(0), 0);
}

#[test]
fn only_one_server_owns_the_pipe_and_many_clients_can_come_one_after_another() {
    let (name, rx, _) = start("twice");
    assert!(pipe::PipeServer::bind(&name).is_err(), "the name is taken");
    for i in 0..3 {
        pipe::send_all(
            &name,
            &[Msg::Hello {
                loader_version: "0".into(),
                client_version: None,
                text_hash: format!("h{i}"),
                pid: i,
                detect_ms: 0,
            }],
        )
        .unwrap();
        let line = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(line.contains(&format!("hash h{i}")), "{line}");
    }
}

#[test]
fn no_server_is_a_plain_error_for_the_client() {
    let e = pipe::connect(&pipe_name("nobody")).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn the_pipe_is_named_after_the_user() {
    let sid = pipe::current_user_sid().unwrap();
    assert!(sid.starts_with("S-1-5-"), "{sid}");
    assert_eq!(pipe::current_pipe_name().unwrap(), ipc::pipe_name(&sid));
}
