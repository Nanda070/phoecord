//! Throwaway tool for stage 1.6.3: inspect a PE image the way the loader's scanner would.
//!
//!   aob-probe strings <exe> <needle>            list ASCII strings containing <needle>
//!   aob-probe xrefs   <exe> <needle>            list `lea reg,[rip+rel32]` references to them
//!   aob-probe sig     <exe> <needle> [pre] [post]  draft + verify AOB signatures around each xref
//!   aob-probe check   <exe> "<pattern>"         count matches of a pattern in .text
//!
//! Uses `phoecord_loader::aob::Pattern` for all matching, so a signature that passes here
//! behaves the same in the loader. Reads only the file given; never touches a running process.

use phoecord_loader::aob::Pattern;

struct Section {
    name: String,
    va: u32,
    vsize: u32,
    raw: u32,
    rawsize: u32,
}

struct Pe {
    image_base: u64,
    data: Vec<u8>,
    sections: Vec<Section>,
}

fn u16le(d: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([d[o], d[o + 1]]) as u32
}
fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

impl Pe {
    fn load(path: &str) -> Pe {
        let data = std::fs::read(path).expect("read exe");
        let pe = u32le(&data, 0x3c) as usize;
        assert_eq!(&data[pe..pe + 4], b"PE\0\0");
        let nsec = u16le(&data, pe + 6) as usize;
        let opt = u16le(&data, pe + 20) as usize;
        let mut o = pe + 24 + opt;
        let mut sections = Vec::new();
        for _ in 0..nsec {
            let name = String::from_utf8_lossy(&data[o..o + 8])
                .trim_end_matches('\0')
                .to_string();
            sections.push(Section {
                name,
                vsize: u32le(&data, o + 8),
                va: u32le(&data, o + 12),
                rawsize: u32le(&data, o + 16),
                raw: u32le(&data, o + 20),
            });
            o += 40;
        }
        let image_base = u64::from_le_bytes(data[pe + 24 + 24..pe + 24 + 32].try_into().unwrap());
        Pe {
            image_base,
            data,
            sections,
        }
    }
    fn section(&self, name: &str) -> &Section {
        self.sections
            .iter()
            .find(|s| s.name == name)
            .expect("section")
    }
    fn bytes(&self, s: &Section) -> &[u8] {
        &self.data[s.raw as usize..(s.raw + s.rawsize.min(s.vsize.max(1))) as usize]
    }
    fn text(&self) -> &[u8] {
        self.bytes(self.section(".text"))
    }
    /// (rva, string) for every printable ASCII run of >= 6 bytes in the non-code sections.
    fn strings(&self, needle: &str) -> Vec<(u32, String, String)> {
        let mut out = Vec::new();
        for s in self.sections.iter().filter(|s| s.name != ".text") {
            let b = self.bytes(s);
            let mut i = 0;
            while i < b.len() {
                let st = i;
                while i < b.len() && (0x20..0x7f).contains(&b[i]) {
                    i += 1;
                }
                if i - st >= 6 {
                    let t = String::from_utf8_lossy(&b[st..i]).to_string();
                    let mut from = 0;
                    while let Some(k) = t[from..].find(needle) {
                        out.push((s.va + (st + from + k) as u32, s.name.clone(), t.clone()));
                        from += k + needle.len().max(1);
                    }
                }
                i += 1;
            }
        }
        out
    }
    /// Any instruction with a rip-relative operand (modrm mod=00 rm=101) reaching `target`;
    /// handles 0, 1 and 4 trailing immediate bytes. Returns the offset of the modrm byte.
    fn rip_xrefs(&self, target: u32) -> Vec<usize> {
        let ts = self.section(".text");
        let t = self.text();
        let mut v = Vec::new();
        for i in 1..t.len().saturating_sub(4) {
            if (t[i - 1] & 0xc7) != 0x05 {
                continue;
            }
            let rel = u32le(t, i) as i32 as i64;
            for imm in [0i64, 1, 4] {
                if ts.va as i64 + i as i64 + 4 + imm + rel == target as i64 {
                    v.push(i - 1);
                    break;
                }
            }
        }
        v
    }
    /// `core::panic::Location` structs (file ptr, len, line, col) whose file path contains `needle`:
    /// (struct rva, file path, line, col).
    fn locations(&self, needle: &str) -> Vec<(u32, String, u32, u32)> {
        let mut out = Vec::new();
        for s in self.sections.iter().filter(|s| s.name != ".text") {
            let b = self.bytes(s);
            let mut i = 0;
            while i + 24 <= b.len() {
                let ptr = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
                let len = u64::from_le_bytes(b[i + 8..i + 16].try_into().unwrap()) as usize;
                if ptr > self.image_base && (8..300).contains(&len) {
                    let rva = (ptr - self.image_base) as u32;
                    if let Some(fs) = self
                        .sections
                        .iter()
                        .find(|x| rva >= x.va && rva + len as u32 <= x.va + x.rawsize)
                    {
                        let off = (fs.raw + rva - fs.va) as usize;
                        if let Ok(path) = std::str::from_utf8(&self.data[off..off + len]) {
                            if path.contains(needle) && path.ends_with(".rs") {
                                out.push((
                                    s.va + i as u32,
                                    path.rsplit("\\")
                                        .take(3)
                                        .collect::<Vec<_>>()
                                        .into_iter()
                                        .rev()
                                        .collect::<Vec<_>>()
                                        .join("/"),
                                    u32le(b, i + 16),
                                    u32le(b, i + 20),
                                ));
                            }
                        }
                    }
                }
                i += 8;
            }
        }
        out
    }
    /// Offsets in .text (and the RVA of the instruction) of `lea r64,[rip+rel32]` hitting `target`.
    fn lea_xrefs(&self, target: u32) -> Vec<usize> {
        let ts = self.section(".text");
        let t = self.text();
        let mut v = Vec::new();
        for i in 0..t.len().saturating_sub(7) {
            if (t[i] == 0x48 || t[i] == 0x4c) && t[i + 1] == 0x8d && (t[i + 2] & 0xc7) == 0x05 {
                let rel = u32le(t, i + 3) as i32 as i64;
                let next = ts.va as i64 + i as i64 + 7;
                if next + rel == target as i64 {
                    v.push(i);
                }
            }
        }
        v
    }
    /// Pattern of text[from..to], wildcarding rel32 operands of lea-rip, call, jmp and jcc near.
    fn draft(&self, from: usize, to: usize) -> String {
        let t = self.text();
        let mut wild = vec![false; to - from];
        let mut i = from;
        while i + 5 <= to {
            let b = t[i];
            if (b == 0xe8 || b == 0xe9) && i + 5 <= to {
                for k in 1..5 {
                    wild[i + k - from] = true;
                }
                i += 5;
            } else if b == 0x0f && i + 6 <= to && (0x80..0x90).contains(&t[i + 1]) {
                for k in 2..6 {
                    wild[i + k - from] = true;
                }
                i += 6;
            } else if b == 0x0f
                && i + 7 <= to
                && matches!(t[i + 1], 0x10 | 0x11 | 0x28 | 0x29)
                && (t[i + 2] & 0xc7) == 0x05
            {
                for k in 3..7 {
                    wild[i + k - from] = true;
                }
                i += 7;
            } else if (b == 0x48 || b == 0x4c)
                && i + 7 <= to
                && t[i + 1] == 0x8d
                && (t[i + 2] & 0xc7) == 0x05
                || (b == 0x48 || b == 0x4c)
                    && i + 7 <= to
                    && t[i + 1] == 0x8b
                    && (t[i + 2] & 0xc7) == 0x05
            {
                for k in 3..7 {
                    wild[i + k - from] = true;
                }
                i += 7;
            } else {
                i += 1;
            }
        }
        (from..to)
            .map(|j| {
                if wild[j - from] {
                    "??".to_string()
                } else {
                    format!("{:02X}", t[j])
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: aob-probe strings|xrefs|sig|check <exe> <needle|pattern> [pre] [post]");
        std::process::exit(2);
    }
    let pe = Pe::load(&a[2]);
    match a[1].as_str() {
        "strings" => {
            for (rva, sec, s) in pe.strings(&a[3]) {
                println!("{rva:08x} {sec:<7} {s}");
            }
        }
        "xrefs" | "sig" => {
            let pre: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(24);
            let post: usize = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(24);
            let ts = pe.section(".text");
            for (rva, _, s) in pe.strings(&a[3]) {
                let xs = pe.lea_xrefs(rva);
                println!("string @{rva:08x} {s:?}: {} xref(s)", xs.len());
                for x in xs {
                    println!("  xref .text+{x:#x} (rva {:#x})", ts.va as usize + x);
                    if a[1] == "sig" {
                        let from = x.saturating_sub(pre);
                        let to = (x + 7 + post).min(pe.text().len());
                        let p = pe.draft(from, to);
                        let n = Pattern::parse(&p).map(|p| p.find_all(pe.text()).count());
                        println!("  sig ({} bytes) matches={n:?}: {p}", to - from);
                    }
                }
            }
        }
        "locs" | "locsig" => {
            // a[3] = file needle, optional a[4] = line filter (0 = any), a[5]/a[6] = pre/post
            let line: u32 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(0);
            let pre: usize = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(24);
            let post: usize = a.get(6).and_then(|x| x.parse().ok()).unwrap_or(24);
            let ts = pe.section(".text");
            for (rva, f, l, c) in pe.locations(&a[3]) {
                if line != 0 && l != line {
                    continue;
                }
                let xs = pe.lea_xrefs(rva);
                println!("Location @{rva:08x} {f}:{l}:{c}: {} xref(s)", xs.len());
                for x in xs {
                    println!("  xref .text+{x:#x} (rva {:#x})", ts.va as usize + x);
                    if a[1] == "locsig" {
                        let from = x.saturating_sub(pre);
                        let to = (x + 7 + post).min(pe.text().len());
                        let p = pe.draft(from, to);
                        let n = Pattern::parse(&p).map(|p| p.find_all(pe.text()).count());
                        println!("  sig ({} bytes) matches={n:?}: {p}", to - from);
                    }
                }
            }
        }
        "ptrs" | "ptrsig" => {
            // a[3] = string needle; lists absolute pointers to it in data sections (static tables
            // of &str) and `lea` references to those slots (or up to 16 bytes before, the table base).
            let pre: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(24);
            let post: usize = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(24);
            let ts = pe.section(".text");
            for (srva, _, _) in pe.strings(&a[3]) {
                let want = (pe.image_base + srva as u64).to_le_bytes();
                for sec in pe.sections.iter().filter(|s| s.name != ".text") {
                    let b = pe.bytes(sec);
                    for i in (0..b.len().saturating_sub(8)).step_by(8) {
                        if b[i..i + 8] == want {
                            let slot = sec.va + i as u32;
                            println!(
                                "string @{srva:08x} pointed to from {} slot @{slot:08x}",
                                sec.name
                            );
                            for back in [0u32, 8, 16, 24] {
                                for x in pe.rip_xrefs(slot - back) {
                                    println!(
                                        "  lea to slot-{back}: .text+{x:#x} (rva {:#x})",
                                        ts.va as usize + x
                                    );
                                    if a[1] == "ptrsig" {
                                        let from = x.saturating_sub(pre);
                                        let to = (x + 7 + post).min(pe.text().len());
                                        let p = pe.draft(from, to);
                                        let n = Pattern::parse(&p)
                                            .map(|p| p.find_all(pe.text()).count());
                                        println!("  sig ({} bytes) matches={n:?}: {p}", to - from);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "check" => {
            let p = Pattern::parse(&a[3]).expect("pattern");
            let hits: Vec<_> = p.find_all(pe.text()).collect();
            println!("{} match(es) {:x?}", hits.len(), hits);
        }
        _ => unreachable!(),
    }
}
