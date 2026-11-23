//! `phoecord log [-n N]`: print the newest entries of the message logger's JSON-lines file.

use crate::text;
use anyhow::Result;
use phoecord_plugins::message_logger::LOG_FILE;
use phoecord_proxy::config::Paths;

pub fn run(paths: &Paths, n: usize) -> Result<()> {
    let file = paths.plugin_dir("message_logger").join(LOG_FILE);
    let body = std::fs::read_to_string(&file).unwrap_or_default();
    let lines = tail(&body, n);
    if lines.is_empty() {
        println!("{}", text::log_empty(&file.display().to_string()));
    }
    for l in lines {
        println!("{}", format_entry(l));
    }
    Ok(())
}

/// The last `n` non-empty lines.
fn tail(body: &str, n: usize) -> Vec<&str> {
    let all: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(n)..].to_vec()
}

/// One human-readable line for one log entry (a broken line is shown as it is).
fn format_entry(line: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return line.to_string();
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let place = if s("guild").is_empty() {
        text::log_channel(&s("channel"))
    } else {
        text::log_guild(&s("guild"), &s("channel"))
    };
    let attachments: Vec<&str> = v
        .get("attachments")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let tail = if attachments.is_empty() {
        String::new()
    } else {
        format!("  [{}]", attachments.join(", "))
    };
    match s("kind").as_str() {
        "edited" => format!(
            "{}  ✏ {} in {place}: {:?} → {:?}{tail}",
            s("time"),
            s("author"),
            s("was"),
            s("content")
        ),
        _ => format!(
            "{}  🗑 {} in {place}: {:?}{tail}",
            s("time"),
            s("author"),
            s("content")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_newest_lines() {
        let body = "a\n\nb\nc\n";
        assert_eq!(tail(body, 2), vec!["b", "c"]);
        assert_eq!(tail(body, 10), vec!["a", "b", "c"]);
        assert!(tail("", 5).is_empty());
    }

    #[test]
    fn entries_are_readable() {
        let del = r#"{"time":"2026-01-01T00:00:00Z","kind":"deleted","channel":"9","guild":null,"author":"bob","content":"hi","was":null,"attachments":[]}"#;
        assert_eq!(
            format_entry(del),
            "2026-01-01T00:00:00Z  🗑 bob in channel 9: \"hi\""
        );
        let edit = r#"{"time":"t","kind":"edited","channel":"9","guild":"7","author":"bob","content":"b","was":"a","attachments":["u"]}"#;
        assert_eq!(
            format_entry(edit),
            "t  ✏ bob in guild 7, channel 9: \"a\" → \"b\"  [u]"
        );
        assert_eq!(format_entry("not json"), "not json");
    }
}
