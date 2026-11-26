//! Start a Discord client that sends its traffic through PhoeCord.
//!
//! Which client: `--channel` names one; without it, the only installed client is used, and with
//! several installed [`ChooseError::Several`] asks the user to pick, so the main client is never run
//! through the proxy by accident. Desktop Phoenix is a different kind of client (its own folder and
//! updater, a native TLS stack that needs PhoeCord's CA in the system store): it is started only by
//! an explicit `--channel phoenix` and never counts as "the only installed client", so adding it
//! to a machine does not change what a bare `phoecord launch` does.

use crate::Channel;
use crate::discover::{Client, Source};
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

/// No Discord client was found; the CLI shows a localized message for this error.
#[derive(Debug)]
pub struct NoDiscord;

impl std::fmt::Display for NoDiscord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no Discord found; pass the command: phoecord launch -- <path to Discord>")
    }
}

impl std::error::Error for NoDiscord {}

/// Why no single client could be picked.
#[derive(Debug)]
pub enum ChooseError {
    /// `--channel` named a channel that is not installed.
    NotInstalled(Channel),
    /// No channel given and several are installed: `(channel, folder)` of each.
    Several(Vec<(Channel, PathBuf)>),
}

impl std::fmt::Display for ChooseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChooseError::NotInstalled(c) => write!(f, "Discord {} is not installed", c.name()),
            ChooseError::Several(list) => {
                write!(f, "several Discord clients are installed; pass --channel:")?;
                for (c, root) in list {
                    write!(f, " {} ({})", c.name(), root.display())?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ChooseError {}

/// `stable`, `ptb`, `canary` or `phoenix`.
pub fn parse_channel(s: &str) -> Option<Channel> {
    match s.to_ascii_lowercase().as_str() {
        "stable" => Some(Channel::Stable),
        "ptb" => Some(Channel::Ptb),
        "canary" => Some(Channel::Canary),
        "phoenix" => Some(Channel::Phoenix),
        _ => None,
    }
}

/// The client to start. `clients` is what [`crate::discover::discover`] found (native packages sort
/// before Flatpak and Snap, so the first of a channel wins). Phoenix is picked only when `want` is
/// [`Channel::Phoenix`]; with no `want` it is left out of the count (see the module docs).
pub fn choose(clients: &[Client], want: Option<Channel>) -> Result<Client> {
    let mut by_channel: Vec<&Client> = Vec::new();
    for c in clients {
        if !by_channel.iter().any(|b| b.channel == c.channel) {
            by_channel.push(c);
        }
    }
    match want {
        Some(ch) => by_channel
            .into_iter()
            .find(|c| c.channel == ch)
            .cloned()
            .ok_or_else(|| ChooseError::NotInstalled(ch).into()),
        None => {
            by_channel.retain(|c| c.channel != Channel::Phoenix);
            match by_channel.as_slice() {
                [] => Err(NoDiscord.into()),
                [one] => Ok((*one).clone()),
                many => Err(ChooseError::Several(
                    many.iter().map(|c| (c.channel, c.root.clone())).collect(),
                )
                .into()),
            }
        }
    }
}

/// What answers on the proxy's address.
#[derive(Debug, PartialEq, Eq)]
pub enum Occupant {
    /// Nothing listens there.
    Nobody,
    /// A PhoeCord proxy, with its version.
    PhoeCord(String),
    /// Something else holds the port.
    Other,
}

/// `phoecord <version>` from an answer to `GET /__phoecord`.
fn parse_identity(answer: &str) -> Option<String> {
    let (head, body) = answer.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") {
        return None;
    }
    let version = body.trim().strip_prefix("phoecord ")?;
    (!version.is_empty() && !version.contains(char::is_whitespace)).then(|| version.to_string())
}

/// Ask whoever listens on `addr` (a loopback `host:port`) whether it is a PhoeCord proxy.
pub fn probe_proxy(addr: &str) -> Occupant {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let timeout = Duration::from_secs(2);
    let Some(sock) = addr
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(|a: &SocketAddr| a.ip().is_loopback()))
    else {
        // Not a loopback address: the proxy never serves it, so there is nothing to reuse.
        return Occupant::Other;
    };
    let Ok(mut s) = TcpStream::connect_timeout(&sock, timeout) else {
        return Occupant::Nobody;
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let ask = format!(
        "GET {} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n",
        phoecord_proxy::server::IDENTITY_PATH
    );
    let mut answer = Vec::new();
    // Read what comes (a stalled or non-HTTP peer ends at the timeout): it is not ours then.
    if s.write_all(ask.as_bytes()).is_ok() {
        let _ = s.take(4096).read_to_end(&mut answer);
    }
    match parse_identity(&String::from_utf8_lossy(&answer)) {
        Some(v) => Occupant::PhoeCord(v),
        None => Occupant::Other,
    }
}

fn proxy_env(cmd: &mut Command, url: &str) {
    // Native (non-Chromium) clients usually honour these instead of --proxy-server.
    for k in ["HTTPS_PROXY", "https_proxy"] {
        cmd.env(k, url);
    }
    cmd.env("NO_PROXY", "localhost,127.0.0.1,::1");
}

/// The `Discord*.app` folder that holds `exe` (`…/Discord.app/Contents/MacOS/Discord`).
fn app_bundle(exe: &std::path::Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(PathBuf::from)
}

/// macOS: start `app` through LaunchServices (`open`), so the client is its own responsible process.
/// A child of PhoeCord.app would make TCC treat its reads of `~/Library/Application Support/discord`
/// as PhoeCord.app reaching into another app's data, and deny them. `open` returns at once; the
/// environment goes with `--env` because `open` does not pass its own on.
fn mac_open_command(app: &std::path::Path, url: &str, flag: &str) -> Command {
    let mut c = Command::new("open");
    c.args(["-n", "-a"]).arg(app);
    for (k, v) in [
        ("HTTPS_PROXY", url),
        ("HTTP_PROXY", url),
        ("NO_PROXY", "localhost,127.0.0.1,::1"),
    ] {
        c.arg("--env").arg(format!("{k}={v}"));
    }
    c.args(["--args", flag]);
    c
}

/// The command that starts `client` with `--proxy-server` pointing at `addr`.
pub fn client_command(client: &Client, addr: &str) -> Command {
    let url = format!("http://{addr}");
    let flag = format!("--proxy-server={url}");
    let file = client
        .exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let update = client.root.parent().map(|p| p.join("Update.exe"));
    let mut cmd = match (client.source, update) {
        // Phoenix has no Update.exe and no app-x.y.z folders: start its executable, from its own
        // folder as its Start-menu shortcut does. A plain launch: used when there is no loader
        // to inject (`phoenix::injected` starts it otherwise, 2.2.8).
        (Source::Native, _) if client.channel == Channel::Phoenix => {
            let mut c = Command::new(&client.exe);
            c.arg(&flag).current_dir(&client.root);
            c
        }
        // Windows: Squirrel's Update.exe starts the newest app-x.y.z folder.
        (Source::Native, Some(u)) if file.ends_with(".exe") && u.is_file() => {
            let mut c = Command::new(u);
            c.args(["--processStart", &file, "--process-start-args", &flag]);
            c
        }
        (Source::Flatpak, _) => {
            let id = if client.channel == Channel::Canary {
                "com.discordapp.DiscordCanary"
            } else {
                "com.discordapp.Discord"
            };
            let mut c = Command::new("flatpak");
            c.args(["run", id, &flag]);
            c
        }
        (Source::Snap, _) => {
            let mut c = Command::new("snap");
            c.args(["run", "discord", &flag]);
            c
        }
        (Source::Native, _) if cfg!(target_os = "macos") && app_bundle(&client.exe).is_some() => {
            let app = app_bundle(&client.exe).unwrap_or_default();
            // The environment is in the arguments; nothing more to set on `open` itself.
            let mut c = mac_open_command(&app, &url, &flag);
            phoecord_proxy::process::hide_console(&mut c);
            return c;
        }
        _ => {
            let mut c = Command::new(&client.exe);
            c.arg(&flag);
            c
        }
    };
    proxy_env(&mut cmd, &url);
    // Update.exe and the clients are GUI programs; no console must flash when the window starts us.
    phoecord_proxy::process::hide_console(&mut cmd);
    cmd
}

/// The command for `args` (an explicit program and its arguments) or, with none, for the client
/// picked from `clients` by `channel`; told to use the proxy at `addr`.
pub fn command(
    args: &[String],
    channel: Option<Channel>,
    clients: &[Client],
    addr: &str,
) -> Result<Command> {
    if let Some((prog, rest)) = args.split_first() {
        let url = format!("http://{addr}");
        let mut c = Command::new(prog);
        c.args(rest).arg(format!("--proxy-server={url}"));
        proxy_env(&mut c, &url);
        return Ok(c);
    }
    Ok(client_command(&choose(clients, channel)?, addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(channel: Channel, source: Source, root: &str, exe: &str) -> Client {
        Client {
            channel,
            source,
            root: root.into(),
            exe: exe.into(),
            version: None,
        }
    }

    fn native(channel: Channel) -> Client {
        client(channel, Source::Native, "/apps/x", "/apps/x/discord")
    }

    fn args(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// Serve `answer` to one connection on a loopback port; returns the port.
    fn one_shot(answer: String) -> u16 {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(answer.as_bytes());
            }
        });
        port
    }

    #[test]
    fn a_phoecord_proxy_is_recognised_by_its_answer() {
        let ours = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: 14\r\n\r\n{}",
            phoecord_proxy::server::identity_body()
        );
        let port = one_shot(ours);
        match probe_proxy(&format!("127.0.0.1:{port}")) {
            Occupant::PhoeCord(v) => assert_eq!(v, env!("CARGO_PKG_VERSION")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_program_on_the_port_is_not_mistaken_for_phoecord() {
        for answer in [
            "HTTP/1.1 200 OK\r\n\r\nhello\r\n",
            "HTTP/1.1 405 Method Not Allowed\r\n\r\nphoecord 1\r\n",
            "",
        ] {
            let port = one_shot(answer.to_string());
            assert_eq!(probe_proxy(&format!("127.0.0.1:{port}")), Occupant::Other);
        }
    }

    #[test]
    fn a_free_port_has_nobody_and_a_remote_address_is_never_reused() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert_eq!(probe_proxy(&format!("127.0.0.1:{port}")), Occupant::Nobody);
        assert_eq!(probe_proxy("192.0.2.1:18463"), Occupant::Other);
    }

    #[test]
    fn channel_names() {
        assert_eq!(parse_channel("Canary"), Some(Channel::Canary));
        assert_eq!(parse_channel("ptb"), Some(Channel::Ptb));
        assert_eq!(parse_channel("stable"), Some(Channel::Stable));
        assert_eq!(parse_channel("Phoenix"), Some(Channel::Phoenix));
        assert_eq!(parse_channel("electron"), None);
    }

    #[test]
    fn phoenix_is_started_only_when_asked_for() {
        let all = [native(Channel::Stable), native(Channel::Phoenix)];
        // Asked for by name, it is the one chosen, even with Stable installed.
        assert_eq!(
            choose(&all, Some(Channel::Phoenix)).unwrap().channel,
            Channel::Phoenix
        );
        // It does not make the bare `launch` ambiguous, and it is never the default.
        assert_eq!(choose(&all, None).unwrap().channel, Channel::Stable);
        let e = choose(&[native(Channel::Phoenix)], None).unwrap_err();
        assert!(e.downcast_ref::<NoDiscord>().is_some());
        // Not installed is an error, not a fallback to another channel.
        let e = choose(&[native(Channel::Stable)], Some(Channel::Phoenix)).unwrap_err();
        assert!(matches!(
            e.downcast_ref::<ChooseError>(),
            Some(ChooseError::NotInstalled(Channel::Phoenix))
        ));
        // Several others plus Phoenix: the list the user is shown has no Phoenix in it.
        let many = [
            native(Channel::Stable),
            native(Channel::Canary),
            native(Channel::Phoenix),
        ];
        match choose(&many, None)
            .unwrap_err()
            .downcast_ref::<ChooseError>()
        {
            Some(ChooseError::Several(l)) => assert_eq!(l.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn phoenix_starts_its_executable_from_its_own_folder() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("DiscordPhoenix").join("app");
        std::fs::create_dir_all(&app).unwrap();
        // Even a stray Update.exe next to the folder is not used: Phoenix has no Squirrel.
        std::fs::write(t.path().join("DiscordPhoenix").join("Update.exe"), b"").unwrap();
        let exe = app.join("Discord.exe");
        let c = client(
            Channel::Phoenix,
            Source::Native,
            &app.to_string_lossy(),
            &exe.to_string_lossy(),
        );
        let cmd = client_command(&c, "127.0.0.1:18463");
        assert_eq!(cmd.get_program(), exe.as_os_str());
        assert_eq!(args(&cmd), ["--proxy-server=http://127.0.0.1:18463"]);
        assert_eq!(cmd.get_current_dir(), Some(app.as_path()));
        let env = |k: &str| {
            cmd.get_envs()
                .find(|(n, _)| *n == k)
                .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(
            env("HTTPS_PROXY").as_deref(),
            Some("http://127.0.0.1:18463")
        );
        assert_eq!(env("NO_PROXY").as_deref(), Some("localhost,127.0.0.1,::1"));
    }

    #[test]
    fn explicit_channel_is_used_even_when_several_are_installed() {
        let all = [native(Channel::Stable), native(Channel::Canary)];
        assert_eq!(
            choose(&all, Some(Channel::Canary)).unwrap().channel,
            Channel::Canary
        );
    }

    #[test]
    fn a_channel_that_is_not_installed_is_an_error_not_a_fallback() {
        let all = [native(Channel::Stable)];
        let e = choose(&all, Some(Channel::Canary)).unwrap_err();
        assert!(matches!(
            e.downcast_ref::<ChooseError>(),
            Some(ChooseError::NotInstalled(Channel::Canary))
        ));
    }

    #[test]
    fn one_client_is_used_without_a_flag() {
        let all = [native(Channel::Ptb)];
        assert_eq!(choose(&all, None).unwrap().channel, Channel::Ptb);
    }

    #[test]
    fn native_and_flatpak_copies_of_one_channel_count_as_one() {
        let all = [
            native(Channel::Stable),
            client(Channel::Stable, Source::Flatpak, "/f", "/f/Discord"),
        ];
        assert_eq!(choose(&all, None).unwrap().source, Source::Native);
    }

    #[test]
    fn several_clients_without_a_flag_never_default_to_stable() {
        let all = [native(Channel::Stable), native(Channel::Canary)];
        let e = choose(&all, None).unwrap_err();
        match e.downcast_ref::<ChooseError>() {
            Some(ChooseError::Several(l)) => {
                let names: Vec<_> = l.iter().map(|(c, _)| c.name()).collect();
                assert_eq!(names, ["stable", "canary"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_installed_is_no_discord() {
        let e = choose(&[], None).unwrap_err();
        assert!(e.downcast_ref::<NoDiscord>().is_some());
    }

    #[test]
    fn windows_client_starts_through_update_exe() {
        let t = tempfile::tempdir().unwrap();
        let canary = t.path().join("DiscordCanary");
        let root = canary.join("app-1.0.1");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(canary.join("Update.exe"), b"").unwrap();
        let c = client(
            Channel::Canary,
            Source::Native,
            &root.to_string_lossy(),
            &root.join("DiscordCanary.exe").to_string_lossy(),
        );
        let cmd = client_command(&c, "127.0.0.1:18463");
        assert!(cmd.get_program().to_string_lossy().ends_with("Update.exe"));
        assert_eq!(
            args(&cmd),
            [
                "--processStart",
                "DiscordCanary.exe",
                "--process-start-args",
                "--proxy-server=http://127.0.0.1:18463"
            ]
        );
    }

    #[test]
    fn explicit_command_keeps_working() {
        let cmd = command(
            &["/opt/d/discord".into(), "--x".into()],
            None,
            &[],
            "127.0.0.1:1",
        )
        .unwrap();
        assert_eq!(args(&cmd), ["--x", "--proxy-server=http://127.0.0.1:1"]);
    }

    #[test]
    fn macos_client_starts_through_open_with_env_in_args() {
        let exe =
            std::path::Path::new("/Applications/Discord Canary.app/Contents/MacOS/Discord Canary");
        let app = app_bundle(exe).unwrap();
        assert_eq!(
            app,
            std::path::Path::new("/Applications/Discord Canary.app")
        );
        let flag = "--proxy-server=http://127.0.0.1:18463";
        let cmd = mac_open_command(&app, "http://127.0.0.1:18463", flag);
        assert_eq!(cmd.get_program().to_string_lossy(), "open");
        assert_eq!(
            args(&cmd),
            [
                "-n",
                "-a",
                "/Applications/Discord Canary.app",
                "--env",
                "HTTPS_PROXY=http://127.0.0.1:18463",
                "--env",
                "HTTP_PROXY=http://127.0.0.1:18463",
                "--env",
                "NO_PROXY=localhost,127.0.0.1,::1",
                "--args",
                flag
            ]
        );
        assert!(app_bundle(std::path::Path::new("/apps/x/discord")).is_none());
    }

    #[test]
    fn linux_native_and_flatpak_commands() {
        let n = client_command(&native(Channel::Stable), "127.0.0.1:1");
        assert_eq!(n.get_program().to_string_lossy(), "/apps/x/discord");
        let f = client_command(
            &client(Channel::Canary, Source::Flatpak, "/f", "/f/DiscordCanary"),
            "127.0.0.1:1",
        );
        assert_eq!(f.get_program().to_string_lossy(), "flatpak");
        assert_eq!(args(&f)[..2], ["run", "com.discordapp.DiscordCanary"]);
    }
}
