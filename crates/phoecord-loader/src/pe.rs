//! Minimal PE (x64 and x86) reader over a byte slice: sections, export names and the
//! `VS_FIXEDFILEINFO` of the version resource. Pure and platform-independent so it is unit-tested
//! on every OS; the loader feeds it the host exe read from disk and the system DLLs.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct PeError(pub &'static str);

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid PE: {}", self.0)
    }
}

impl std::error::Error for PeError {}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
}

pub struct Pe<'a> {
    data: &'a [u8],
    pub sections: Vec<Section>,
    /// `(rva, size)` of data directories 0 (export) and 2 (resource).
    export_dir: (u32, u32),
    resource_dir: (u32, u32),
}

fn u16_at(d: &[u8], o: usize) -> Result<u16, PeError> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(PeError("truncated"))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, PeError> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(PeError("truncated"))
}

impl<'a> Pe<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Pe<'a>, PeError> {
        if data.get(..2) != Some(b"MZ") {
            return Err(PeError("no MZ header"));
        }
        let pe = u32_at(data, 0x3c)? as usize;
        if data.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(PeError("no PE signature"));
        }
        let nsec = u16_at(data, pe + 6)? as usize;
        let opt_size = u16_at(data, pe + 20)? as usize;
        let opt = pe + 24;
        let dirs = match u16_at(data, opt)? {
            0x20b => opt + 112,
            0x10b => opt + 96,
            _ => return Err(PeError("unknown optional header")),
        };
        let dir = |i: usize| -> Result<(u32, u32), PeError> {
            Ok((u32_at(data, dirs + i * 8)?, u32_at(data, dirs + i * 8 + 4)?))
        };
        let mut sections = Vec::with_capacity(nsec);
        for i in 0..nsec {
            let s = opt + opt_size + i * 40;
            let raw = data.get(s..s + 8).ok_or(PeError("truncated"))?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(8);
            sections.push(Section {
                name: String::from_utf8_lossy(&raw[..end]).into_owned(),
                virtual_size: u32_at(data, s + 8)?,
                virtual_address: u32_at(data, s + 12)?,
                raw_size: u32_at(data, s + 16)?,
                raw_offset: u32_at(data, s + 20)?,
            });
        }
        Ok(Pe {
            data,
            sections,
            export_dir: dir(0)?,
            resource_dir: dir(2)?,
        })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Bytes of a section as stored in the file: `min(raw size, virtual size)`, which is the real
    /// code without file-alignment padding.
    pub fn section_bytes(&self, name: &str) -> Option<&'a [u8]> {
        let s = self.section(name)?;
        let start = s.raw_offset as usize;
        let len = s.raw_size.min(s.virtual_size) as usize;
        self.data.get(start..start.checked_add(len)?)
    }

    fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find_map(|s| {
            let span = s.virtual_size.max(s.raw_size);
            (rva >= s.virtual_address && rva - s.virtual_address < span)
                .then(|| (rva - s.virtual_address + s.raw_offset) as usize)
        })
    }

    /// Names of all named exports, in table order.
    pub fn export_names(&self) -> Result<Vec<String>, PeError> {
        let (rva, _) = self.export_dir;
        if rva == 0 {
            return Ok(Vec::new());
        }
        let e = self.rva_to_offset(rva).ok_or(PeError("bad export rva"))?;
        let count = u32_at(self.data, e + 24)? as usize;
        let names = self
            .rva_to_offset(u32_at(self.data, e + 32)?)
            .ok_or(PeError("bad names rva"))?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let n = self
                .rva_to_offset(u32_at(self.data, names + i * 4)?)
                .ok_or(PeError("bad name rva"))?;
            let tail = self.data.get(n..).ok_or(PeError("truncated"))?;
            let len = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or(PeError("unterminated name"))?;
            out.push(String::from_utf8_lossy(&tail[..len]).into_owned());
        }
        Ok(out)
    }

    /// File version `a.b.c.d` from the first `RT_VERSION` resource, if present.
    pub fn file_version(&self) -> Option<String> {
        let (rva, size) = self.resource_dir;
        if rva == 0 {
            return None;
        }
        let base = self.rva_to_offset(rva)?;
        let res = self.data.get(base..base.checked_add(size as usize)?)?;
        // Resource tree: type -> name/id -> language -> data entry. `want` selects an ID entry
        // (only used for the type level); otherwise the first entry is taken.
        let entry = |dir: usize, want: Option<u32>| -> Option<u32> {
            let named = u16_at(res, dir + 12).ok()? as usize;
            let ids = u16_at(res, dir + 14).ok()? as usize;
            (0..named + ids).find_map(|i| {
                let e = dir + 16 + i * 8;
                let id = u32_at(res, e).ok()?;
                let hit = want.is_none_or(|w| i >= named && id == w);
                hit.then(|| u32_at(res, e + 4).ok()).flatten()
            })
        };
        let t = entry(0, Some(16))?; // RT_VERSION
        let n = entry((t & 0x7fff_ffff) as usize, None)?;
        let l = entry((n & 0x7fff_ffff) as usize, None)?;
        let data_entry = l as usize;
        let data_rva = u32_at(res, data_entry).ok()?;
        let data_size = u32_at(res, data_entry + 4).ok()? as usize;
        let off = self.rva_to_offset(data_rva)?;
        let blob = self.data.get(off..off.checked_add(data_size)?)?;
        // VS_FIXEDFILEINFO starts with the signature 0xFEEF04BD.
        let sig = [0xBD, 0x04, 0xEF, 0xFE];
        let at = blob.windows(4).position(|w| w == sig)?;
        let ms = u32_at(blob, at + 8).ok()?;
        let ls = u32_at(blob, at + 12).ok()?;
        Some(format!(
            "{}.{}.{}.{}",
            ms >> 16,
            ms & 0xffff,
            ls >> 16,
            ls & 0xffff
        ))
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    fn align(n: usize) -> usize {
        n.div_ceil(0x200) * 0x200
    }

    fn put32(v: &mut [u8], at: usize, x: u32) {
        v[at..at + 4].copy_from_slice(&x.to_le_bytes());
    }

    /// A resource directory with one ID entry.
    fn res_dir(id: u32, child: u32) -> Vec<u8> {
        let mut v = vec![0u8; 24];
        v[14..16].copy_from_slice(&1u16.to_le_bytes());
        put32(&mut v, 16, id);
        put32(&mut v, 20, child);
        v
    }

    /// Builds a tiny PE64 (RVA == file offset) with `.text`, optional exports and an optional
    /// version resource.
    pub fn build_pe(
        text: &[u8],
        exports: &[&str],
        version: Option<(u16, u16, u16, u16)>,
    ) -> Vec<u8> {
        let text_off = 0x200;
        let text_len = align(text.len().max(1));
        let edata_off = text_off + text_len;
        let mut edata = Vec::new();
        if !exports.is_empty() {
            let n = exports.len();
            let funcs = edata_off + 40;
            let names = funcs + 4 * n;
            let ords = names + 4 * n;
            let mut sp = ords + 2 * n;
            edata.resize(40 + 10 * n, 0);
            put32(&mut edata, 24, n as u32);
            put32(&mut edata, 28, funcs as u32);
            put32(&mut edata, 32, names as u32);
            put32(&mut edata, 36, ords as u32);
            let mut strs = Vec::new();
            for (i, name) in exports.iter().enumerate() {
                put32(&mut edata, 40 + 4 * i, text_off as u32);
                put32(&mut edata, 40 + 4 * n + 4 * i, sp as u32);
                let o = 40 + 8 * n + 2 * i;
                edata[o..o + 2].copy_from_slice(&(i as u16).to_le_bytes());
                strs.extend_from_slice(name.as_bytes());
                strs.push(0);
                sp += name.len() + 1;
            }
            edata.extend_from_slice(&strs);
        }
        let edata_len = align(edata.len());
        let rsrc_off = edata_off + edata_len;
        let mut rsrc = Vec::new();
        if let Some((a, b, c, d)) = version {
            // dirs at 0x00, 0x18, 0x30; data entry at 0x48; VS_FIXEDFILEINFO at 0x58.
            rsrc.extend(res_dir(16, 0x8000_0018));
            rsrc.extend(res_dir(1, 0x8000_0030));
            rsrc.extend(res_dir(0x409, 0x48));
            rsrc.resize(0x58, 0);
            put32(&mut rsrc, 0x48, (rsrc_off + 0x58) as u32);
            put32(&mut rsrc, 0x4c, 52);
            let mut ffi = vec![0u8; 52];
            put32(&mut ffi, 0, 0xFEEF_04BD);
            put32(&mut ffi, 8, ((a as u32) << 16) | b as u32);
            put32(&mut ffi, 12, ((c as u32) << 16) | d as u32);
            rsrc.extend(ffi);
        }
        let mut f = vec![0u8; 0x200];
        f[0..2].copy_from_slice(b"MZ");
        put32(&mut f, 0x3c, 0x80);
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        f[0x86..0x88].copy_from_slice(&3u16.to_le_bytes());
        f[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        let opt = 0x98;
        f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        let dirs = opt + 112;
        if !exports.is_empty() {
            put32(&mut f, dirs, edata_off as u32);
            put32(&mut f, dirs + 4, edata.len() as u32);
        }
        if version.is_some() {
            put32(&mut f, dirs + 16, rsrc_off as u32);
            put32(&mut f, dirs + 20, rsrc.len() as u32);
        }
        let sec = opt + 0xf0;
        let mut put = |i: usize, name: &[u8], off: usize, len: usize, real: usize| {
            let s = sec + i * 40;
            f[s..s + name.len()].copy_from_slice(name);
            put32(&mut f, s + 8, real as u32);
            put32(&mut f, s + 12, off as u32);
            put32(&mut f, s + 16, len as u32);
            put32(&mut f, s + 20, off as u32);
        };
        put(0, b".text", text_off, text_len, text.len());
        put(1, b".edata", edata_off, edata_len, edata.len());
        put(2, b".rsrc", rsrc_off, align(rsrc.len()), rsrc.len());
        let mut body = text.to_vec();
        body.resize(text_len, 0);
        f.extend(body);
        edata.resize(edata_len, 0);
        f.extend(edata);
        rsrc.resize(align(rsrc.len()), 0);
        f.extend(rsrc);
        f
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::build_pe;
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(Pe::parse(b"hello").is_err());
        assert!(Pe::parse(&[]).is_err());
    }

    #[test]
    fn reads_sections_exports_and_version() {
        let bytes = build_pe(&[1, 2, 3, 4], &["Alpha", "Beta"], Some((1, 2, 345, 6)));
        let pe = Pe::parse(&bytes).unwrap();
        assert_eq!(pe.section_bytes(".text").unwrap(), &[1, 2, 3, 4]);
        assert_eq!(pe.export_names().unwrap(), ["Alpha", "Beta"]);
        assert_eq!(pe.file_version().as_deref(), Some("1.2.345.6"));
    }

    #[test]
    fn missing_directories_are_not_errors() {
        let bytes = build_pe(&[0x90], &[], None);
        let pe = Pe::parse(&bytes).unwrap();
        assert!(pe.export_names().unwrap().is_empty());
        assert_eq!(pe.file_version(), None);
    }
}
