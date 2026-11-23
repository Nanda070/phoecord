//! Start a Discord client that sends its traffic through PhoeCord.
//!
//! Which client: `--channel` names one; without it, the only installed client is used, and with
//! several installed [`ChooseError::Several`] asks the user to pick, so the main client is never run
//! through the proxy by accident.

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

/// `stable`, `ptb` or `canary`.
pub fn parse_channel(s: &str) -> Option<Channel> {
    match s.to_ascii_lowercase().as_str() {
        "stable" => Some(Channel::Stable),
        "ptb" => Some(Channel::Ptb),
        "canary" => Some(Channel::Canary),
        _ => None,
    }
}

/// The client to start. `clients` is what [`crate::discover::discover`] found (native packages sort
/// before Flatpak and Snap, so the first of a channel wins).
pub fn choose(clients: &[Client], want: Option<Channel>) -> Result<Client> {
    let mut by_channel: Vec<&Client> = Vec::new();
    for c in clients.iter().filter(|c| c.channel != Channel::Phoenix) {
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
        None => match by_channel.as_slice() {
            [] => Err(NoDiscord.into()),
            [one] => Ok((*one).clone()),
            many => Err(ChooseError::Several(
                many.iter().map(|c| (c.channel, c.root.clone())).collect(),
            )
            .into()),
        },
    }
}

fn proxy_env(cmd: &mut Command, url: &str) {
    // Native (non-Chromium) clients usually honour these instead of --proxy-server.
    for k in ["HTTPS_PROXY", "https_proxy"] {
        cmd.env(k, url);
    }
    cmd.env("NO_PROXY", "localhost,127.0.0.1,::1");
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
        _ => {
            let mut c = Command::new(&client.exe);
            c.arg(&flag);
            c
        }
    };
    proxy_env(&mut cmd, &url);
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

    #[test]
    fn channel_names() {
        assert_eq!(parse_channel("Canary"), Some(Channel::Canary));
        assert_eq!(parse_channel("ptb"), Some(Channel::Ptb));
        assert_eq!(parse_channel("stable"), Some(Channel::Stable));
        assert_eq!(parse_channel("phoenix"), None);
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
        // Phoenix is not a client the proxy can serve yet.
        let e = choose(&[native(Channel::Phoenix)], None).unwrap_err();
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
