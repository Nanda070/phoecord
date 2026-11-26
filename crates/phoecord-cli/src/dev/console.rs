//! `phoecord dev console` (task 1.3.9): attaches to the running proxy's dev endpoint
//! (`phoecord-proxy::dev`) with the token from the data folder, prints what the plugins log and
//! what the chain does with each event, and runs commands typed on stdin.

use crate::tf;
use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use phoecord_i18n::Localizer;
use phoecord_proxy::config::{Config, Paths};
use phoecord_proxy::dev::{PREFIX, token_path};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

#[derive(ClapArgs)]
pub struct Args {
    /// Show only events whose name or route contains this text (e.g. `MESSAGE_CREATE`, `/channels`)
    #[arg(long, value_name = "TEXT")]
    pub filter: Option<String>,
    /// Show only this plugin's log lines and runs
    #[arg(long, value_name = "ID")]
    pub plugin: Option<String>,
}

/// What is shown. Logs are filtered by plugin only; runs by plugin and by event name or route.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Filter {
    pub event: Option<String>,
    pub plugin: Option<String>,
}

impl Filter {
    pub fn allows(&self, entry: &Value) -> bool {
        let field = |k: &str| entry.get(k).and_then(Value::as_str).unwrap_or("");
        if let Some(p) = &self.plugin
            && !field("plugin").eq_ignore_ascii_case(p)
        {
            return false;
        }
        match (entry.get("type").and_then(Value::as_str), &self.event) {
            (Some("run"), Some(text)) => field("event")
                .to_ascii_lowercase()
                .contains(&text.to_ascii_lowercase()),
            _ => true,
        }
    }
}

/// `HH:MM:SS.mmm` (UTC) of a Unix time in milliseconds.
pub fn clock(ms: u64) -> String {
    let day = ms % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        day / 3_600_000,
        day / 60_000 % 60,
        day / 1000 % 60,
        day % 1000
    )
}

fn duration(micros: u64) -> String {
    if micros < 1000 {
        format!("{micros}µs")
    } else {
        format!("{:.1}ms", micros as f64 / 1000.0)
    }
}

/// One stream entry as a line, or `None` when it is not shown (`hello`, `ping`, filtered out).
pub fn render(entry: &Value, filter: &Filter) -> Option<String> {
    let text = |k: &str| entry.get(k).and_then(Value::as_str).unwrap_or("");
    let time = clock(entry.get("ts").and_then(Value::as_u64).unwrap_or(0));
    match entry.get("type").and_then(Value::as_str)? {
        "log" if filter.allows(entry) => Some(format!(
            "{time}  log           {}: {}",
            text("plugin"),
            text("message")
        )),
        "run" if filter.allows(entry) => Some(format!(
            "{time}  {:<13} {} -> {}: {} ({})",
            text("kind"),
            text("event"),
            text("plugin"),
            text("verdict"),
            duration(entry.get("micros").and_then(Value::as_u64).unwrap_or(0)),
        )),
        _ => None,
    }
}

/// What the user typed.
#[derive(Debug, PartialEq)]
pub enum Input {
    Empty,
    Quit,
    Help,
    /// Set (or with `None` clear) the event filter.
    SetFilter(Option<String>),
    SetPlugin(Option<String>),
    /// A command for the proxy.
    Remote(Value),
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Usage(&'static str),
    Unknown(String),
}

pub fn parse(line: &str) -> Result<Input, ParseError> {
    let line = line.trim();
    let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let rest = rest.trim();
    let plugin_cmd = |cmd: &'static str| {
        if rest.is_empty() || rest.contains(char::is_whitespace) {
            Err(ParseError::Usage(match cmd {
                "toggle" => "toggle <plugin>",
                "reload" => "reload <plugin>",
                "settings" => "settings <plugin>",
                _ => "kv <plugin>",
            }))
        } else {
            Ok(Input::Remote(json!({"cmd": cmd, "plugin": rest})))
        }
    };
    match word.to_ascii_lowercase().as_str() {
        "" => Ok(Input::Empty),
        "quit" | "exit" | "q" => Ok(Input::Quit),
        "help" | "?" => Ok(Input::Help),
        "plugins" | "ls" => Ok(Input::Remote(json!({"cmd": "plugins"}))),
        "events" => Ok(Input::Remote(json!({"cmd": "events"}))),
        "toggle" => plugin_cmd("toggle"),
        "reload" => plugin_cmd("reload"),
        "settings" => plugin_cmd("settings"),
        "kv" => plugin_cmd("kv"),
        "filter" => Ok(Input::SetFilter(
            (!rest.is_empty()).then(|| rest.to_string()),
        )),
        "plugin" => Ok(Input::SetPlugin(
            (!rest.is_empty()).then(|| rest.to_string()),
        )),
        "replay" => {
            // The event may hold spaces (`POST /channels/1/messages`): the plugin is the last word.
            match rest.rsplit_once(char::is_whitespace) {
                Some((event, plugin)) if !event.trim().is_empty() => Ok(Input::Remote(
                    json!({"cmd": "replay", "event": event.trim(), "plugin": plugin}),
                )),
                _ => Err(ParseError::Usage("replay <event> <plugin>")),
            }
        }
        other => Err(ParseError::Unknown(other.to_string())),
    }
}

/// An answer of the proxy as text for the user.
pub fn show(l: &Localizer, cmd: &Value, answer: &Value) -> String {
    if answer.get("ok") != Some(&Value::Bool(true)) {
        let why = answer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return format!("{}: {why}", l.t("cli-error-prefix"));
    }
    let result = &answer["result"];
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let state = |on: bool| {
        l.t(if on {
            "cli-dev-state-on"
        } else {
            "cli-dev-state-off"
        })
    };
    match cmd["cmd"].as_str().unwrap_or("") {
        "plugins" => result
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|p| {
                        let crashed = p["crashed"].as_bool().unwrap_or(false);
                        let on = if crashed {
                            l.t("cli-dev-state-crashed")
                        } else {
                            state(p["enabled"].as_bool().unwrap_or(false))
                        };
                        format!(
                            "{:<28} {:<8} {:<8} {}",
                            s(p, "id"),
                            on,
                            l.t(&format!("cli-dev-source-{}", s(p, "source"))),
                            s(p, "version")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "toggle" => tf(
            l,
            "cli-dev-toggled",
            &[
                ("plugin", &s(result, "plugin")),
                (
                    "state",
                    &state(result["enabled"].as_bool().unwrap_or(false)),
                ),
            ],
        ),
        "reload" => tf(l, "cli-dev-reloaded", &[("plugin", &s(result, "plugin"))]),
        "replay" => tf(
            l,
            "cli-dev-replayed",
            &[
                ("event", &s(result, "event")),
                ("plugin", &s(result, "plugin")),
                ("verdict", &s(result, "verdict")),
                ("sends", &result["dropped_sends"].to_string()),
                ("locals", &result["dropped_locals"].to_string()),
            ],
        ),
        _ => serde_json::to_string_pretty(result).unwrap_or_default(),
    }
}

/// The proxy's address and token, from the config and the data folder.
fn endpoint(paths: &Paths, l: &Localizer) -> Result<(String, String)> {
    let listen = if paths.config.exists() {
        Config::load(&paths.config)?.listen
    } else {
        Config::default().listen
    };
    let file = token_path(&paths.data);
    let token = std::fs::read_to_string(&file).with_context(|| {
        tf(
            l,
            "cli-dev-err-no-token",
            &[("path", &file.display().to_string())],
        )
    })?;
    Ok((listen, token.trim().to_string()))
}

/// Send one request to the proxy's dev endpoint on its own connection.
async fn send(listen: &str, req: Request<Full<Bytes>>) -> Result<Response<Incoming>> {
    let tcp = TcpStream::connect(listen).await?;
    tcp.set_nodelay(true).ok();
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tcp)).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(tx.send_request(req).await?)
}

fn request(token: &str, method: &str, path: &str, body: Option<&Value>) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(format!("{PREFIX}{path}"))
        .header("host", "phoecord")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(
            body.map(Value::to_string).unwrap_or_default(),
        )))
        .expect("a valid request")
}

/// Run one command and return the proxy's answer.
pub async fn call(listen: &str, token: &str, cmd: &Value) -> Result<Value> {
    let res = send(listen, request(token, "POST", "/command", Some(cmd))).await?;
    let status = res.status();
    let body = res.into_body().collect().await?.to_bytes();
    if status != StatusCode::OK {
        bail!("{status}");
    }
    Ok(serde_json::from_slice(&body)?)
}

/// Open the stream; the response body carries NDJSON lines.
pub async fn open_stream(listen: &str, token: &str) -> Result<Response<Incoming>> {
    send(listen, request(token, "GET", "/stream", None)).await
}

pub fn run(args: Args, paths: &Paths, l: &Localizer) -> Result<()> {
    let (listen, token) = endpoint(paths, l)?;
    let filter = Filter {
        event: args.filter,
        plugin: args.plugin,
    };
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(session(listen, token, filter, l));
    // A blocked read of stdin must not keep the process alive.
    rt.shutdown_background();
    result
}

async fn session(listen: String, token: String, filter: Filter, l: &Localizer) -> Result<()> {
    let res = match open_stream(&listen, &token).await {
        Ok(res) => res,
        Err(e) => bail!(tf(
            l,
            "cli-dev-err-connect",
            &[("listen", &listen), ("why", &e.to_string())]
        )),
    };
    match res.status() {
        StatusCode::OK => {}
        StatusCode::UNAUTHORIZED => bail!(l.t("cli-dev-err-token")),
        other => bail!(tf(
            l,
            "cli-dev-err-status",
            &[("status", &other.to_string())]
        )),
    }
    let filter = Arc::new(Mutex::new(filter));
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();
    let reader = {
        let (filter, listen) = (filter.clone(), listen.clone());
        let l = Localizer::new(l.language());
        tokio::spawn(async move {
            let mut body = res.into_body();
            let mut pending = Vec::<u8>::new();
            while let Some(Ok(frame)) = body.frame().await {
                let Some(data) = frame.data_ref() else {
                    continue;
                };
                pending.extend_from_slice(data);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    let Ok(entry) = serde_json::from_slice::<Value>(&line) else {
                        continue;
                    };
                    if entry["type"] == "hello" {
                        println!(
                            "{}",
                            tf(
                                &l,
                                "cli-dev-attached",
                                &[
                                    ("version", entry["version"].as_str().unwrap_or("?")),
                                    ("listen", &listen),
                                    (
                                        "count",
                                        &entry["plugins"]
                                            .as_array()
                                            .map_or(0, Vec::len)
                                            .to_string()
                                    ),
                                ]
                            )
                        );
                    } else if let Some(text) = render(&entry, &filter.lock().unwrap()) {
                        println!("{text}");
                    }
                }
            }
            let _ = done_tx.send(());
        })
    };

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdin_open = true;
    loop {
        tokio::select! {
            _ = &mut done_rx => {
                println!("{}", l.t("cli-dev-detached"));
                break;
            }
            _ = tokio::signal::ctrl_c() => break,
            line = lines.next_line(), if stdin_open => {
                let Ok(Some(line)) = line else {
                    // No more input (a pipe): keep showing the stream until Ctrl+C.
                    stdin_open = false;
                    continue;
                };
                match parse(&line) {
                    Ok(Input::Empty) => {}
                    Ok(Input::Quit) => break,
                    Ok(Input::Help) => println!("{}", l.t("cli-dev-help").trim_end()),
                    Ok(Input::SetFilter(f)) => {
                        let msg = match &f {
                            Some(t) => tf(l, "cli-dev-filter-set", &[("filter", t)]),
                            None => l.t("cli-dev-filter-cleared"),
                        };
                        filter.lock().unwrap().event = f;
                        println!("{msg}");
                    }
                    Ok(Input::SetPlugin(p)) => {
                        let msg = match &p {
                            Some(t) => tf(l, "cli-dev-plugin-set", &[("plugin", t)]),
                            None => l.t("cli-dev-plugin-cleared"),
                        };
                        filter.lock().unwrap().plugin = p;
                        println!("{msg}");
                    }
                    Ok(Input::Remote(cmd)) => match call(&listen, &token, &cmd).await {
                        Ok(answer) => println!("{}", show(l, &cmd, &answer)),
                        Err(e) => eprintln!("{}: {e:#}", l.t("cli-error-prefix")),
                    },
                    Err(ParseError::Usage(usage)) => {
                        println!("{}", tf(l, "cli-dev-usage", &[("usage", usage)]));
                    }
                    Err(ParseError::Unknown(name)) => {
                        println!("{}", tf(l, "cli-dev-unknown-command", &[("name", &name)]));
                    }
                }
            }
        }
    }
    reader.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
    use phoecord_plugin_runtime::Chain;
    use phoecord_proxy::{Proxy, server};

    fn l() -> Localizer {
        Localizer::new("en")
    }

    fn run_entry(event: &str, plugin: &str) -> Value {
        json!({"type":"run","ts":3_723_004u64,"kind":"dispatch","event":event,"plugin":plugin,"verdict":"pass","micros":1500u64})
    }

    #[test]
    fn lines_show_time_event_plugin_verdict_and_timing() {
        let f = Filter::default();
        assert_eq!(
            render(&run_entry("MESSAGE_CREATE", "p"), &f).unwrap(),
            "01:02:03.004  dispatch      MESSAGE_CREATE -> p: pass (1.5ms)"
        );
        let log = json!({"type":"log","ts":0u64,"plugin":"p","message":"hi"});
        assert_eq!(
            render(&log, &f).unwrap(),
            "00:00:00.000  log           p: hi"
        );
        assert!(render(&json!({"type":"ping"}), &f).is_none());
        assert!(render(&json!({"type":"hello"}), &f).is_none());
        assert_eq!(duration(250), "250µs");
    }

    #[test]
    fn the_filter_matches_event_name_or_route_and_plugin() {
        let by_event = Filter {
            event: Some("channels".into()),
            plugin: None,
        };
        assert!(render(&run_entry("POST /channels/1/messages", "p"), &by_event).is_some());
        assert!(render(&run_entry("MESSAGE_CREATE", "p"), &by_event).is_none());
        // A log line has no event: only the plugin filter applies to it.
        let log = json!({"type":"log","ts":0u64,"plugin":"p","message":"hi"});
        assert!(render(&log, &by_event).is_some());
        let by_plugin = Filter {
            event: None,
            plugin: Some("Q".into()),
        };
        assert!(render(&log, &by_plugin).is_none());
        assert!(render(&run_entry("X", "q"), &by_plugin).is_some());
    }

    #[test]
    fn commands_are_parsed() {
        assert_eq!(parse("  "), Ok(Input::Empty));
        assert_eq!(parse("quit"), Ok(Input::Quit));
        assert_eq!(
            parse("toggle no_track"),
            Ok(Input::Remote(json!({"cmd":"toggle","plugin":"no_track"})))
        );
        assert_eq!(
            parse("replay POST /channels/1/messages my_plugin"),
            Ok(Input::Remote(json!({
                "cmd":"replay","event":"POST /channels/1/messages","plugin":"my_plugin"
            })))
        );
        assert_eq!(
            parse("filter MESSAGE_"),
            Ok(Input::SetFilter(Some("MESSAGE_".into())))
        );
        assert_eq!(parse("filter"), Ok(Input::SetFilter(None)));
        assert_eq!(parse("reload"), Err(ParseError::Usage("reload <plugin>")));
        assert_eq!(
            parse("replay MESSAGE_CREATE"),
            Err(ParseError::Usage("replay <event> <plugin>"))
        );
        assert_eq!(parse("fly"), Err(ParseError::Unknown("fly".into())));
    }

    #[test]
    fn answers_become_sentences_and_errors_stay_errors() {
        let cmd = json!({"cmd":"toggle","plugin":"p"});
        let ok = json!({"ok":true,"result":{"plugin":"p","enabled":false}});
        assert_eq!(show(&l(), &cmd, &ok), "Plugin p is now off.");
        let err = json!({"ok":false,"error":"no plugin p"});
        assert!(show(&l(), &cmd, &err).ends_with("no plugin p"));
        let list = json!({"ok":true,"result":[
            {"id":"a","enabled":true,"crashed":false,"source":"builtin","version":"1"},
            {"id":"b","enabled":true,"crashed":true,"source":"package","version":"2"}]});
        let text = show(&l(), &json!({"cmd":"plugins"}), &list);
        assert!(text.contains("on") && text.contains("crashed"), "{text}");
    }

    struct Echo(Manifest);

    impl Plugin for Echo {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, k: &EventKind) -> bool {
            matches!(k, EventKind::Dispatch { .. })
        }
        fn on_event(&self, h: &mut dyn Host, _: Event<'_>) -> Verdict {
            h.log("hello from echo");
            Verdict::Pass
        }
    }

    /// The console's own client code against a real proxy listener: stream and commands.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_client_talks_to_a_real_endpoint() {
        let dir = std::env::temp_dir().join(format!("phoecord-cli-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config: dir.join("config.toml"),
            data: dir.join("data"),
        };
        let mut cfg = Config::default();
        cfg.set_enabled("echo", true).unwrap();
        cfg.save(&paths.config).unwrap();
        let manifest = Manifest::new("echo", "Echo", "1", String::new());
        let chain = Chain::new(vec![Box::new(Echo(manifest))]);
        let proxy = Proxy::new(
            Paths {
                config: paths.config.clone(),
                data: paths.data.clone(),
            },
            cfg,
            chain,
            false,
        )
        .unwrap();
        proxy.publish_dev_token().unwrap();
        let listener = server::bind("127.0.0.1:0").await.unwrap();
        let listen = listener.local_addr().unwrap().to_string();
        tokio::spawn(server::serve(proxy.clone(), listener));

        let token = std::fs::read_to_string(token_path(&paths.data)).unwrap();
        assert!(open_stream(&listen, "wrong").await.unwrap().status() == StatusCode::UNAUTHORIZED);

        let mut res = open_stream(&listen, &token).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let mut payload = Val::map();
        payload.set("op", Val::Int(0));
        payload.set("t", Val::Str("TYPING_START".into()));
        proxy.on_dispatch(payload);
        let mut text = String::new();
        while !text.contains("\"run\"") {
            let frame =
                tokio::time::timeout(std::time::Duration::from_secs(5), res.body_mut().frame())
                    .await
                    .expect("a frame")
                    .expect("not ended")
                    .unwrap();
            if let Some(d) = frame.data_ref() {
                text.push_str(&String::from_utf8_lossy(d));
            }
        }
        let lines: Vec<Value> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let run = lines.iter().find(|v| v["type"] == "run").unwrap();
        assert!(
            render(run, &Filter::default())
                .unwrap()
                .contains("TYPING_START -> echo: pass")
        );

        let cmd = json!({"cmd":"toggle","plugin":"echo"});
        let answer = call(&listen, &token, &cmd).await.unwrap();
        assert_eq!(show(&l(), &cmd, &answer), "Plugin echo is now off.");
        assert!(call(&listen, "wrong", &cmd).await.is_err());
    }
}
