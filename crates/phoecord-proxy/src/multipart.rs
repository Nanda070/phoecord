//! `multipart/form-data` bodies of message sends and edits (task 2.4.2, item a).
//!
//! The client sends a message with files as `multipart/form-data`: a `payload_json` part (the same
//! JSON a plain send has, with `attachments: [{id, filename, ...}]`) and one `files[n]` part per
//! file. Plugins see `payload_json` as the request body, like any JSON send. When one changes it,
//! [`Form::rebuild`] writes the body again with the same boundary: the new `payload_json`, and each
//! `files[n]` part byte for byte, its `filename` taken from the attachment with `id` n when a
//! plugin renamed it there. File bytes are never parsed or changed.
//!
//! The parser works on bytes (files are binary) and refuses anything it does not understand
//! (`None`), in which case the proxy forwards the original body untouched.

use phoecord_core::Val;

/// One part: its raw header block (without the blank line) and its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub headers: Vec<u8>,
    pub data: Vec<u8>,
}

impl Part {
    /// A `Content-Disposition` parameter (`name`, `filename`), unquoted.
    fn disposition(&self, key: &str) -> Option<String> {
        let text = String::from_utf8_lossy(&self.headers);
        let line = text.split("\r\n").find(|l| {
            l.split_once(':')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("content-disposition"))
        })?;
        let (_, value) = line.split_once(':')?;
        value.split(';').skip(1).find_map(|p| {
            let (k, v) = p.trim().split_once('=')?;
            k.trim()
                .eq_ignore_ascii_case(key)
                .then(|| unquote(v.trim()))
        })
    }

    pub fn name(&self) -> Option<String> {
        self.disposition("name")
    }

    pub fn filename(&self) -> Option<String> {
        self.disposition("filename")
    }

    /// The same part with `filename` set to `name` in its `Content-Disposition` (a `filename*`
    /// parameter is dropped, so the plain one counts).
    fn renamed(&self, name: &str) -> Part {
        let text = String::from_utf8_lossy(&self.headers);
        let lines: Vec<String> = text
            .split("\r\n")
            .map(|l| {
                let Some((k, v)) = l.split_once(':') else {
                    return l.to_string();
                };
                if !k.trim().eq_ignore_ascii_case("content-disposition") {
                    return l.to_string();
                }
                let mut params: Vec<String> = Vec::new();
                for (i, p) in v.split(';').enumerate() {
                    let p = p.trim();
                    if i == 0 {
                        params.push(p.to_string());
                        continue;
                    }
                    let key = p.split_once('=').map_or(p, |(k, _)| k).trim();
                    if key.eq_ignore_ascii_case("filename*") {
                        continue;
                    }
                    if key.eq_ignore_ascii_case("filename") {
                        params.push(format!("filename=\"{}\"", quote(name)));
                    } else {
                        params.push(p.to_string());
                    }
                }
                format!("{}: {}", k, params.join("; "))
            })
            .collect();
        Part {
            headers: lines.join("\r\n").into_bytes(),
            data: self.data.clone(),
        }
    }
}

fn unquote(v: &str) -> String {
    match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => v.to_string(),
    }
}

/// A file name safe inside a quoted header parameter: no quotes, backslashes or line breaks.
fn quote(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '"' | '\\' | '\r' | '\n' => '_',
            c => c,
        })
        .collect()
}

/// A parsed `multipart/form-data` body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub boundary: String,
    pub parts: Vec<Part>,
}

/// The boundary of a `multipart/form-data` content type.
pub fn boundary(content_type: &str) -> Option<String> {
    let ct = content_type.trim();
    if !ct.to_ascii_lowercase().starts_with("multipart/form-data") {
        return None;
    }
    ct.split(';').skip(1).find_map(|p| {
        let (k, v) = p.trim().split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| v.trim().trim_matches('"').to_string())
            .filter(|b| !b.is_empty() && b.len() <= 70)
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

impl Form {
    pub fn parse(content_type: &str, body: &[u8]) -> Option<Form> {
        let boundary = boundary(content_type)?;
        let delim = format!("--{boundary}").into_bytes();
        let next = {
            let mut d = b"\r\n".to_vec();
            d.extend_from_slice(&delim);
            d
        };
        // The first delimiter starts the body or follows a preamble line.
        let mut pos = if body.starts_with(&delim) {
            delim.len()
        } else {
            find(body, &next, 0)? + next.len()
        };
        let mut parts = Vec::new();
        loop {
            let rest = body.get(pos..)?;
            if rest.starts_with(b"--") {
                return Some(Form { boundary, parts });
            }
            // Transport padding (spaces, tabs) may follow a delimiter before its CRLF.
            let line_end = find(body, b"\r\n", pos)?;
            if body[pos..line_end]
                .iter()
                .any(|b| *b != b' ' && *b != b'\t')
            {
                return None;
            }
            let start = line_end + 2;
            let end = find(body, &next, start)?;
            let part = &body[start..end];
            let (headers, data) = match find(part, b"\r\n\r\n", 0) {
                Some(h) => (part[..h].to_vec(), part[h + 4..].to_vec()),
                // A part with no headers starts with the blank line.
                None if part.starts_with(b"\r\n") => (Vec::new(), part[2..].to_vec()),
                None => return None,
            };
            parts.push(Part { headers, data });
            pos = end + next.len();
        }
    }

    fn payload_index(&self) -> Option<usize> {
        self.parts
            .iter()
            .position(|p| p.name().as_deref() == Some("payload_json"))
    }

    /// The `payload_json` part as JSON.
    pub fn payload_json(&self) -> Option<Val> {
        let p = &self.parts[self.payload_index()?];
        serde_json::from_slice(&p.data).ok().map(Val::from_json)
    }

    /// The body again with `payload` as `payload_json` and each `files[n]` part renamed to the
    /// `filename` of `payload.attachments[id == n]` when that differs. `None` when the form has no
    /// `payload_json` part to replace.
    pub fn rebuild(&self, payload: &Val) -> Option<Vec<u8>> {
        let at = self.payload_index()?;
        let json = serde_json::to_vec(&payload.to_json()).ok()?;
        let mut out = Vec::new();
        for (i, part) in self.parts.iter().enumerate() {
            let part = if i == at {
                Part {
                    headers: part.headers.clone(),
                    data: json.clone(),
                }
            } else {
                match file_index(part).and_then(|n| attachment_name(payload, n)) {
                    Some(name) if part.filename().as_deref() != Some(name.as_str()) => {
                        part.renamed(&name)
                    }
                    _ => part.clone(),
                }
            };
            out.extend_from_slice(b"--");
            out.extend_from_slice(self.boundary.as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(&part.headers);
            out.extend_from_slice(b"\r\n\r\n");
            out.extend_from_slice(&part.data);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"--");
        out.extend_from_slice(self.boundary.as_bytes());
        out.extend_from_slice(b"--\r\n");
        Some(out)
    }
}

/// `n` of a `files[n]` part.
fn file_index(part: &Part) -> Option<u64> {
    part.name()?
        .strip_prefix("files[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// The `filename` of the attachment with `id` n (a number or a numeric string, as clients send).
fn attachment_name(payload: &Val, n: u64) -> Option<String> {
    payload
        .get("attachments")?
        .as_list()
        .iter()
        .find(|a| {
            a.get("id").is_some_and(|id| {
                id.as_i64().and_then(|i| u64::try_from(i).ok()) == Some(n)
                    || id.as_str().and_then(|s| s.parse().ok()) == Some(n)
            })
        })?
        .str_at(&["filename"])
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CT: &str = "multipart/form-data; boundary=----B1";

    fn body(file: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(
            b"------B1\r\nContent-Disposition: form-data; name=\"payload_json\"\r\n\
              Content-Type: application/json\r\n\r\n\
              {\"content\":\"hi https://x.com/?utm_source=a\",\"attachments\":[{\"id\":0,\"filename\":\"me.png\"}]}\r\n\
              ------B1\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"me.png\"\r\n\
              Content-Type: image/png\r\n\r\n",
        );
        b.extend_from_slice(file);
        b.extend_from_slice(b"\r\n------B1--\r\n");
        b
    }

    /// Binary data with CR, LF, NUL and a near-boundary.
    const FILE: &[u8] = b"\x89PNG\r\n\x1a\n\x00\xff\r\n------B\r\n--";

    #[test]
    fn parses_payload_json_and_binary_files() {
        let f = Form::parse(CT, &body(FILE)).unwrap();
        assert_eq!(f.boundary, "----B1");
        assert_eq!(f.parts.len(), 2);
        assert_eq!(f.parts[1].data, FILE);
        assert_eq!(f.parts[1].filename().as_deref(), Some("me.png"));
        let p = f.payload_json().unwrap();
        assert_eq!(
            p.str_at(&["content"]),
            Some("hi https://x.com/?utm_source=a")
        );
    }

    #[test]
    fn an_unchanged_payload_rebuilds_the_same_parts() {
        let f = Form::parse(CT, &body(FILE)).unwrap();
        let again = f.rebuild(&f.payload_json().unwrap()).unwrap();
        let g = Form::parse(CT, &again).unwrap();
        assert_eq!(g.parts[1], f.parts[1]);
        assert_eq!(g.payload_json(), f.payload_json());
    }

    #[test]
    fn a_changed_payload_and_file_name_reach_the_body() {
        let f = Form::parse(CT, &body(FILE)).unwrap();
        let mut p = f.payload_json().unwrap();
        p.set("content", Val::Str("hi https://x.com/".into()));
        let att = p.get_mut("attachments").unwrap().as_list_mut().unwrap();
        att[0].set("filename", Val::Str("a\"b.png".into()));
        let g = Form::parse(CT, &f.rebuild(&p).unwrap()).unwrap();
        assert_eq!(
            g.payload_json().unwrap().str_at(&["content"]),
            Some("hi https://x.com/")
        );
        assert_eq!(g.parts[1].filename().as_deref(), Some("a_b.png"));
        assert_eq!(g.parts[1].data, FILE, "file bytes untouched");
        assert!(
            String::from_utf8_lossy(&g.parts[1].headers).contains("Content-Type: image/png"),
            "other headers kept"
        );
    }

    #[test]
    fn string_ids_and_filename_star_are_handled() {
        let b = b"--X\r\nContent-Disposition: form-data; name=\"payload_json\"\r\n\r\n\
                  {\"attachments\":[{\"id\":\"1\",\"filename\":\"new.txt\"}]}\r\n\
                  --X\r\nContent-Disposition: form-data; name=\"files[1]\"; filename=\"old.txt\"; filename*=UTF-8''old.txt\r\n\r\n\
                  abc\r\n--X--";
        let f = Form::parse("multipart/form-data; boundary=\"X\"", b).unwrap();
        let g = Form::parse(
            "multipart/form-data; boundary=X",
            &f.rebuild(&f.payload_json().unwrap()).unwrap(),
        )
        .unwrap();
        let h = String::from_utf8_lossy(&g.parts[1].headers).to_string();
        assert!(
            h.contains("filename=\"new.txt\"") && !h.contains("filename*"),
            "{h}"
        );
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        assert!(Form::parse("application/json", &body(FILE)).is_none());
        assert!(Form::parse("multipart/form-data; boundary=zz", &body(FILE)).is_none());
        // Cut before the closing delimiter.
        let b = body(FILE);
        assert!(Form::parse(CT, &b[..b.len() - 12]).is_none());
        // Garbage after a delimiter.
        assert!(Form::parse(CT, b"------B1xx\r\n\r\nx\r\n------B1--").is_none());
        // No payload_json: nothing to rebuild.
        let only_file =
            b"------B1\r\nContent-Disposition: form-data; name=\"files[0]\"\r\n\r\nx\r\n------B1--";
        let f = Form::parse(CT, only_file).unwrap();
        assert!(f.payload_json().is_none());
        assert!(f.rebuild(&Val::map()).is_none());
    }
}
