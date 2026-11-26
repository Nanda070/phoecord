//! The `--help` text, built at run time from the `Localizer` (clap wants `&'static str`, so the derive
//! attributes carry none and everything is set here).

use clap::{Arg, ArgAction, Command};
use phoecord_i18n::Localizer;

/// `args` holds `--lang`, `--help` and `--version`, which every command shares.
pub fn localize(mut cmd: Command, l: &Localizer) -> Command {
    let opts = leak(l.t("cli-help-options"));
    let template = format!(
        "{{about-with-newline}}\n{} {{usage}}\n\n{{all-args}}{{after-help}}",
        l.t("cli-help-usage")
    );
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    cmd = cmd
        .about(l.t("cmd-about"))
        .help_template(template.clone())
        .subcommand_help_heading(leak(l.t("cli-help-commands")))
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .global(true)
                .help(l.t("cli-help-help"))
                .help_heading(opts),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help(l.t("cli-help-version"))
                .help_heading(opts),
        )
        .mut_arg("lang", |a| a.help(l.t("cli-arg-lang")).help_heading(opts));
    for name in names {
        let about = l.t(&format!("cmd-{name}"));

        let args_heading = leak(l.t("cli-help-arguments"));
        cmd = cmd.mut_subcommand(&name, |s| {
            let mut s = s.about(about).help_template(template.clone());
            let ids: Vec<String> = s.get_arguments().map(|a| a.get_id().to_string()).collect();
            for id in ids {
                s = s.mut_arg(&id, |a| {
                    let positional = a.is_positional();
                    let a = if positional {
                        a.help_heading(args_heading)
                    } else {
                        a.help_heading(opts)
                    };
                    if id == "n" {
                        a.help(l.t("cli-arg-log-n")).hide_default_value(true)
                    } else if id == "channel" {
                        a.help(l.t(if name == "setup" {
                            "cli-arg-setup-channel"
                        } else {
                            "cli-arg-channel"
                        }))
                    } else if id == "exe" {
                        a.help(l.t("cli-arg-check-exe"))
                    } else if id == "signatures" {
                        a.help(l.t("cli-arg-check-signatures"))
                            .hide_default_value(true)
                    } else if id == "path" {
                        a.help(l.t("cli-arg-import-path"))
                    } else if id == "accept_terms" {
                        a.help(l.t("cli-arg-accept-terms"))
                    } else if id == "yes" {
                        a.help(l.t("cli-arg-yes"))
                    } else if id == "target" {
                        a.help(l.t("cli-arg-install-target"))
                    } else if id == "id" {
                        a.help(l.t(if name == "update" {
                            "cli-arg-update-id"
                        } else {
                            "cli-arg-remove-id"
                        }))
                    } else if id == "transport" {
                        a.help(l.t("cli-arg-transport"))
                    } else if id == "loader" {
                        a.help(l.t("cli-arg-setup-loader"))
                    } else {
                        a
                    }
                });
            }
            nested(s, &name, l, &template, opts, args_heading)
        });
    }
    cmd
}

/// Second-level commands (`phoecord dev watch`): the about line is `cmd-<parent>-<child>`, an
/// argument's help is `cli-arg-<parent>-<child>-<arg id>`; whatever has no key keeps its English.
fn nested(
    mut cmd: Command,
    parent: &str,
    l: &Localizer,
    template: &str,
    opts: &'static str,
    args_heading: &'static str,
) -> Command {
    let children: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    for child in children {
        cmd = cmd.mut_subcommand(&child, |c| {
            let mut c = c.help_template(template.to_string());
            if let Some(about) = l.try_t(&format!("cmd-{parent}-{child}")) {
                c = c.about(about);
            }
            let ids: Vec<String> = c.get_arguments().map(|a| a.get_id().to_string()).collect();
            for id in ids {
                c = c.mut_arg(&id, |a| {
                    let heading = if a.is_positional() {
                        args_heading
                    } else {
                        opts
                    };
                    let a = a.help_heading(heading);
                    match l.try_t(&format!("cli-arg-{parent}-{child}-{id}")) {
                        Some(text) => a.help(text),
                        None => a,
                    }
                });
            }
            c
        });
    }
    cmd
}

/// `--lang X` or `--lang=X` from the raw arguments, read before clap runs so the help text can be in
/// that language.
pub fn lang_from_args(args: &[String]) -> Option<String> {
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--" {
            return None;
        }
        if a == "--lang" {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix("--lang=") {
            return Some(v.to_string());
        }
    }
    None
}

/// clap keeps headings as `&'static str`; the CLI builds its help once per process, so leaking is fine.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::CommandFactory;

    fn build(lang: &str) -> Command {
        localize(Cli::command(), &Localizer::new(lang))
    }

    #[test]
    fn russian_help_is_translated() {
        let cmd = build("ru");
        let about = |n: &str| {
            cmd.find_subcommand(n)
                .unwrap()
                .get_about()
                .unwrap()
                .to_string()
        };
        assert_eq!(about("plugins"), "Показать плагины и их состояние");
        assert_eq!(about("enable"), "Включить плагин");
        let mut cmd = build("ru");
        let help = cmd.render_help().to_string();
        assert!(help.contains("Использование:"), "{help}");
        assert!(help.contains("Команды"), "{help}");
        for english in [
            "Usage:",
            "Commands:",
            "Options:",
            "Print help",
            "Switch a plugin",
        ] {
            assert!(!help.contains(english), "{english} in {help}");
        }
        // A subcommand's own help too.
        let mut cmd = build("ru");
        let sub = cmd
            .find_subcommand_mut("log")
            .unwrap()
            .render_help()
            .to_string();
        assert!(
            sub.contains("Сколько записей показать (по умолчанию 20)"),
            "{sub}"
        );
        assert!(!sub.contains("How many"), "{sub}");
    }

    #[test]
    fn english_help_keeps_the_english_source() {
        let cmd = build("en");
        assert_eq!(
            cmd.find_subcommand("plugins")
                .unwrap()
                .get_about()
                .unwrap()
                .to_string(),
            "List plugins and whether they are on"
        );
    }

    #[test]
    fn every_subcommand_has_an_about_key() {
        let en = Localizer::new("en");
        for s in Cli::command().get_subcommands() {
            let key = format!("cmd-{}", s.get_name());
            assert!(en.try_t(&key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn lang_flag_is_found_before_clap() {
        let v = |a: &[&str]| lang_from_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            v(&["phoecord", "--lang", "ru", "--help"]),
            Some("ru".into())
        );
        assert_eq!(v(&["phoecord", "plugins", "--lang=ru"]), Some("ru".into()));
        assert_eq!(v(&["phoecord", "launch", "--", "--lang", "ru"]), None);
        assert_eq!(v(&["phoecord"]), None);
    }
}
