//! `phoecord check <exe> [--signatures <dir>]` (task 2.2.6): which signatures of a client build
//! resolve in an exe on disk, and which typed locators of the hook targets find theirs (2.2.6
//! part 2). Read-only; fails (exit code 1) unless every signature resolves and, for a Phoenix exe
//! (`DiscordPhoenix` in its path), every locator finds its target.

use crate::tf;
use anyhow::{Context, Result};
use phoecord_i18n::Localizer;
use phoecord_loader::check::{Report, check};
use phoecord_loader::signatures::ResolveError;
use phoecord_loader::version::ClientKind;
use std::path::Path;

pub fn run(exe: &Path, signatures: &Path, l10n: &Localizer) -> Result<()> {
    let bytes = std::fs::read(exe).with_context(|| {
        tf(
            l10n,
            "cli-check-err-read",
            &[("path", &exe.display().to_string())],
        )
    })?;
    let report = check(&bytes, signatures).with_context(|| {
        tf(
            l10n,
            "cli-check-err",
            &[
                ("exe", &exe.display().to_string()),
                ("signatures", &signatures.display().to_string()),
            ],
        )
    })?;
    for line in lines(&report, l10n) {
        println!("{line}");
    }
    let phoenix = ClientKind::of(&exe.to_string_lossy(), None) == ClientKind::Phoenix;
    if !report.passed() || (phoenix && !report.locators_passed()) {
        anyhow::bail!(l10n.t("cli-check-failed"));
    }
    Ok(())
}

/// The report as printed: the build, skipped files, then the table of signatures.
fn lines(report: &Report, l10n: &Localizer) -> Vec<String> {
    let unknown = l10n.t("cli-check-no-version");
    let mut out = vec![tf(
        l10n,
        "cli-check-build",
        &[
            (
                "version",
                report.version.file_version.as_deref().unwrap_or(&unknown),
            ),
            ("hash", &report.version.text_hash),
        ],
    )];
    for (file, why) in &report.skipped {
        out.push(tf(
            l10n,
            "cli-check-skipped",
            &[("file", file), ("why", &why.to_string())],
        ));
    }
    let Some(table) = &report.table else {
        out.push(l10n.t("cli-check-no-table"));
        out.extend(locator_lines(report, l10n));
        return out;
    };
    out.push(tf(l10n, "cli-check-table", &[("key", table)]));
    let rows: Vec<[String; 3]> = std::iter::once([
        l10n.t("cli-check-head-name"),
        l10n.t("cli-check-head-status"),
        l10n.t("cli-check-head-address"),
    ])
    .chain(report.entries.iter().map(|e| {
        let (status, address) = match &e.result {
            Ok(rva) => (l10n.t("cli-check-found"), format!("{rva:#010x}")),
            Err(why) => (describe(why, l10n), "-".to_string()),
        };
        [e.name.clone(), status, address]
    }))
    .collect();
    let width = |i: usize| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0);
    let (name, status) = (width(0), width(1));
    for r in &rows {
        out.push(format!("  {:<name$}  {:<status$}  {}", r[0], r[1], r[2]));
    }
    out.push(tf(
        l10n,
        "cli-check-summary",
        &[
            ("found", &report.found().to_string()),
            ("total", &report.entries.len().to_string()),
        ],
    ));
    out.extend(locator_lines(report, l10n));
    out
}

/// The typed locators: one row each with what they found (`role 0x…`) or why they missed.
fn locator_lines(report: &Report, l10n: &Localizer) -> Vec<String> {
    if report.locators.is_empty() {
        return Vec::new();
    }
    let mut out = vec![l10n.t("cli-check-locators")];
    let name = report
        .locators
        .iter()
        .map(|l| l.name.len())
        .max()
        .unwrap_or(0);
    for l in &report.locators {
        let detail = match &l.result {
            Ok(found) => {
                let rvas: Vec<String> = found
                    .rvas
                    .iter()
                    .map(|(role, rva)| format!("{role} {rva:#010x}"))
                    .collect();
                format!(
                    "{}  {} ({})",
                    l10n.t("cli-check-found"),
                    rvas.join(", "),
                    found.how
                )
            }
            Err(why) => format!("{}: {why}", l10n.t("cli-check-missed")),
        };
        out.push(format!("  {:<name$}  {detail}", l.name));
    }
    let found = report.locators.iter().filter(|l| l.result.is_ok()).count();
    out.push(tf(
        l10n,
        "cli-check-summary",
        &[
            ("found", &found.to_string()),
            ("total", &report.locators.len().to_string()),
        ],
    ));
    out
}

fn describe(why: &ResolveError, l10n: &Localizer) -> String {
    match why {
        ResolveError::NotFound => l10n.t("cli-check-missed"),
        ResolveError::Ambiguous(n) => tf(l10n, "cli-check-ambiguous", &[("count", &n.to_string())]),
        ResolveError::BadRip => l10n.t("cli-check-bad-rip"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_loader::check::Entry;
    use phoecord_loader::version::ClientVersion;

    fn report(table: Option<&str>, entries: Vec<Entry>) -> Report {
        Report {
            version: ClientVersion {
                file_version: Some("1.0.0.1".into()),
                text_hash: "ab12".into(),
                text_rva: 0x1000,
                text_len: 0,
            },
            table: table.map(str::to_string),
            entries,
            skipped: Vec::new(),
            locators: Vec::new(),
        }
    }

    #[test]
    fn the_table_lists_status_and_address() {
        let entry = |name: &str, result| Entry {
            name: name.into(),
            result,
        };
        let r = report(
            Some("1.0.0.1"),
            vec![
                entry("send", Ok(0x1020)),
                entry("palette", Err(ResolveError::Ambiguous(3))),
            ],
        );
        let text = lines(&r, &Localizer::new("en")).join("\n");
        for part in ["1.0.0.1", "ab12", "send", "0x00001020", "palette", "1"] {
            assert!(text.contains(part), "{part} not in {text}");
        }
    }

    #[test]
    fn a_missing_table_is_said_and_no_rows_follow() {
        let l10n = Localizer::new("en");
        let out = lines(&report(None, Vec::new()), &l10n);
        assert_eq!(out.last(), Some(&l10n.t("cli-check-no-table")));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn locators_get_their_own_rows() {
        use phoecord_loader::check::{Located, LocatorEntry};
        use phoecord_loader::locators::LocatorError;
        let mut r = report(None, Vec::new());
        r.locators = vec![
            LocatorEntry {
                name: "http_send",
                result: Ok(Located {
                    rvas: vec![("send", 0x2000)],
                    how: "masked AOB".into(),
                }),
            },
            LocatorEntry {
                name: "allocator",
                result: Err(LocatorError::Missing {
                    what: "__rust_alloc".into(),
                }),
            },
        ];
        let l10n = Localizer::new("en");
        let text = lines(&r, &l10n).join(
            "
",
        );
        for part in [
            l10n.t("cli-check-locators"),
            "send 0x00002000 (masked AOB)".into(),
            "__rust_alloc: not found".into(),
            "Found 1 of 2".into(),
        ] {
            assert!(text.contains(&part), "{part} not in {text}");
        }
    }
}
