//! Tags and captions inside JPEG files, where Picasa 3.9 puts them (lab check,
//! `docs/picasa-organize-format.md`): XMP `dc:subject` (tags) and `dc:description` (caption),
//! and IPTC 2:25 Keywords / 2:120 Caption in a Photoshop APP13 block, marked UTF-8 (1:90).
//!
//! Writing replaces only those fields: other XMP properties, other IPTC records and other
//! Photoshop resources are kept, and the picture data (from the start-of-scan marker on) is
//! copied unchanged.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::sidecar;

const XMP_SIG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const PS_SIG: &[u8] = b"Photoshop 3.0\0";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Labels {
    pub tags: Vec<String>,
    pub caption: String,
}

/// The JPEG's segments before the picture data: (marker, payload) pairs, and where the
/// picture data starts. None if this is not a JPEG.
fn segments(file: &[u8]) -> Option<(Vec<(u8, &[u8])>, usize)> {
    walk(file, false)
}

/// `lenient`: the buffer may be only the start of the file; return the segments that fit.
fn walk(file: &[u8], lenient: bool) -> Option<(Vec<(u8, &[u8])>, usize)> {
    if !file.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 2;
    loop {
        if i + 4 > file.len() || file[i] != 0xFF {
            return if lenient { Some((out, file.len())) } else { None };
        }
        let marker = file[i + 1];
        if marker == 0xDA {
            return Some((out, i));
        }
        let len = u16::from_be_bytes([file[i + 2], file[i + 3]]) as usize;
        if len < 2 || i + 2 + len > file.len() {
            return if lenient && len >= 2 { Some((out, file.len())) } else { None };
        }
        out.push((marker, &file[i + 4..i + 2 + len]));
        i += 2 + len;
    }
}

// ---------- reading ----------

/// Tags and caption from a JPEG file, or from just its first part (what the scanner reads).
pub fn read(file: &[u8]) -> Labels {
    let Some((segs, _)) = walk(file, true) else { return Labels::default() };
    let mut l = Labels::default();
    // XMP first (what Picasa and most programs read), IPTC when XMP has nothing
    if let Some(x) = segs.iter().find(|(m, p)| *m == 0xE1 && p.starts_with(XMP_SIG)) {
        let text = String::from_utf8_lossy(&x.1[XMP_SIG.len()..]).to_string();
        l.tags = xmp_list(&text, "dc:subject");
        l.caption = xmp_list(&text, "dc:description").into_iter().next().unwrap_or_default();
    }
    if l.tags.is_empty() || l.caption.is_empty() {
        let records = segs.iter().find(|(m, p)| *m == 0xED && p.starts_with(PS_SIG)).and_then(|p| iptc_block(p.1)).map(iptc_records).unwrap_or_default();
        let utf8 = records.iter().any(|(r, d, v)| *r == 1 && *d == 90 && v.as_slice() == b"\x1b%G");
        let s = |v: &[u8]| if utf8 { String::from_utf8_lossy(v).to_string() } else { v.iter().map(|&b| b as char).collect() };
        if l.tags.is_empty() {
            l.tags = records.iter().filter(|(r, d, _)| *r == 2 && *d == 25).map(|(_, _, v)| s(v)).collect();
        }
        if l.caption.is_empty() {
            l.caption = records.iter().find(|(r, d, _)| *r == 2 && *d == 120).map(|(_, _, v)| s(v)).unwrap_or_default();
        }
    }
    l.tags.retain(|t| !t.trim().is_empty());
    l
}

/// The `rdf:li` values inside `<prop>…</prop>`.
fn xmp_list(xmp: &str, prop: &str) -> Vec<String> {
    let Some(start) = xmp.find(&format!("<{prop}>")).or_else(|| xmp.find(&format!("<{prop} "))) else { return Vec::new() };
    let Some(end) = xmp[start..].find(&format!("</{prop}>")) else { return Vec::new() };
    let inner = &xmp[start..start + end];
    let mut out = Vec::new();
    let mut rest = inner;
    while let Some(a) = rest.find("<rdf:li") {
        let Some(gt) = rest[a..].find('>') else { break };
        let body = &rest[a + gt + 1..];
        let Some(close) = body.find("</rdf:li>") else { break };
        out.push(unescape(&body[..close]));
        rest = &body[close..];
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#xA;", "\n").replace("&amp;", "&")
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The IPTC data inside a Photoshop APP13 payload (resource 0x0404).
fn iptc_block(ps: &[u8]) -> Option<&[u8]> {
    resources(ps).into_iter().find(|r| r.0 == 0x0404).map(|r| r.2)
}

/// Photoshop image resources: (id, name bytes incl. padding, data).
fn resources(ps: &[u8]) -> Vec<(u16, &[u8], &[u8])> {
    let mut out = Vec::new();
    let mut i = PS_SIG.len();
    while i + 12 <= ps.len() && &ps[i..i + 4] == b"8BIM" {
        let id = u16::from_be_bytes([ps[i + 4], ps[i + 5]]);
        let nlen = ps[i + 6] as usize;
        let name_total = (1 + nlen).div_ceil(2) * 2;
        let name = &ps[i + 6..i + 6 + name_total];
        let at = i + 6 + name_total;
        if at + 4 > ps.len() {
            break;
        }
        let size = u32::from_be_bytes([ps[at], ps[at + 1], ps[at + 2], ps[at + 3]]) as usize;
        let data_at = at + 4;
        if data_at + size > ps.len() {
            break;
        }
        out.push((id, name, &ps[data_at..data_at + size]));
        i = data_at + size + (size & 1);
    }
    out
}

fn iptc_records(b: &[u8]) -> Vec<(u8, u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 5 <= b.len() && b[i] == 0x1C {
        let (r, d) = (b[i + 1], b[i + 2]);
        let len = u16::from_be_bytes([b[i + 3], b[i + 4]]) as usize;
        if len & 0x8000 != 0 || i + 5 + len > b.len() {
            break; // extended lengths are not used for these fields
        }
        out.push((r, d, b[i + 5..i + 5 + len].to_vec()));
        i += 5 + len;
    }
    out
}

// ---------- writing ----------

fn segment(marker: u8, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() + 2 > 0xFFFF {
        bail!("the tags and caption are too long to fit in the photo");
    }
    let mut s = vec![0xFF, marker];
    s.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    s.extend_from_slice(payload);
    Ok(s)
}

fn new_xmp(l: &Labels) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?> <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"> \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"> \
         <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">{} </rdf:Description> \
         </rdf:RDF> </x:xmpmeta> <?xpacket end=\"w\"?>",
        dc_props(l)
    )
}

fn dc_props(l: &Labels) -> String {
    let mut s = String::new();
    if !l.tags.is_empty() {
        s.push_str(" <dc:subject> <rdf:Bag>");
        for t in &l.tags {
            s.push_str(&format!(" <rdf:li>{}</rdf:li>", escape(t)));
        }
        s.push_str(" </rdf:Bag> </dc:subject>");
    }
    if !l.caption.is_empty() {
        s.push_str(&format!(" <dc:description> <rdf:Alt> <rdf:li xml:lang=\"x-default\">{}</rdf:li> </rdf:Alt> </dc:description>", escape(&l.caption)));
    }
    s
}

/// An existing XMP packet with dc:subject and dc:description replaced (other properties kept).
fn edit_xmp(xmp: &str, l: &Labels) -> Option<String> {
    let mut x = xmp.to_string();
    for prop in ["dc:subject", "dc:description"] {
        // element form <dc:subject>…</dc:subject>
        while let Some(a) = x.find(&format!("<{prop}>")).or_else(|| x.find(&format!("<{prop} "))) {
            let close = format!("</{prop}>");
            let e = x[a..].find(&close)? + a + close.len();
            x.replace_range(a..e, "");
        }
        // attribute form dc:description="…" cannot hold a list; refuse to guess
        if x.contains(&format!("{prop}=\"")) {
            return None;
        }
    }
    let d = x.find("<rdf:Description")?;
    let gt = x[d..].find('>')? + d;
    let self_closing = x.as_bytes()[gt - 1] == b'/';
    let mut open = x[d..gt].trim_end_matches('/').to_string();
    if !open.contains("xmlns:dc=") {
        open.push_str(" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"");
    }
    let props = dc_props(l);
    let replacement = if self_closing { format!("{open}>{props} </rdf:Description>") } else { format!("{open}>{props}") };
    x.replace_range(d..gt + 1, &replacement);
    Some(x)
}

/// A Photoshop APP13 payload with its IPTC tags and caption replaced (other records kept).
fn edit_ps(old: Option<&[u8]>, l: &Labels) -> Vec<u8> {
    let res = old.map(resources).unwrap_or_default();
    let records = res.iter().find(|r| r.0 == 0x0404).map(|r| iptc_records(r.2)).unwrap_or_default();
    let mut iptc = Vec::new();
    let mut put = |r: u8, d: u8, v: &[u8]| {
        iptc.extend_from_slice(&[0x1C, r, d]);
        iptc.extend_from_slice(&(v.len().min(0x7FFF) as u16).to_be_bytes());
        iptc.extend_from_slice(&v[..v.len().min(0x7FFF)]);
    };
    // envelope: keep what was there, make sure the character set says UTF-8
    let env: Vec<_> = records.iter().filter(|(r, d, _)| *r == 1 && *d != 90).collect();
    if env.is_empty() {
        put(1, 0, &[0, 4]);
    }
    for (r, d, v) in env {
        put(*r, *d, v);
    }
    put(1, 90, b"\x1b%G");
    let app: Vec<_> = records.iter().filter(|(r, d, _)| *r == 2 && *d != 25 && *d != 120).collect();
    if app.iter().all(|(_, d, _)| *d != 0) {
        put(2, 0, &[0, 4]);
    }
    for (r, d, v) in app {
        put(*r, *d, v);
    }
    for t in &l.tags {
        put(2, 25, t.as_bytes());
    }
    if !l.caption.is_empty() {
        put(2, 120, l.caption.as_bytes());
    }
    let mut ps = PS_SIG.to_vec();
    let mut wrote_iptc = false;
    let push_res = |ps: &mut Vec<u8>, id: u16, name: &[u8], data: &[u8]| {
        ps.extend_from_slice(b"8BIM");
        ps.extend_from_slice(&id.to_be_bytes());
        ps.extend_from_slice(name);
        ps.extend_from_slice(&(data.len() as u32).to_be_bytes());
        ps.extend_from_slice(data);
        if data.len() & 1 == 1 {
            ps.push(0);
        }
    };
    for (id, name, data) in &res {
        if *id == 0x0404 {
            push_res(&mut ps, 0x0404, name, &iptc);
            wrote_iptc = true;
        } else if *id != 0x0425 {
            // 0x0425 is a checksum of the old IPTC; it would now be wrong, so it is dropped
            push_res(&mut ps, *id, name, data);
        }
    }
    if !wrote_iptc {
        push_res(&mut ps, 0x0404, &[0, 0], &iptc);
    }
    ps
}

/// The file with its tags and caption set to `l`. The picture data is copied unchanged.
pub fn write(file: &[u8], l: &Labels) -> Result<Vec<u8>> {
    let Some((segs, sos)) = segments(file) else { bail!("tags and captions can only be written into JPEG photos") };
    let old_xmp = segs.iter().find(|(m, p)| *m == 0xE1 && p.starts_with(XMP_SIG)).map(|s| String::from_utf8_lossy(&s.1[XMP_SIG.len()..]).to_string());
    let xmp = match &old_xmp {
        Some(x) => match edit_xmp(x, l) {
            Some(x) => x,
            None => bail!("this photo's XMP data is in a form CasaPic cannot edit safely"),
        },
        None => new_xmp(l),
    };
    let old_ps = segs.iter().find(|(m, p)| *m == 0xED && p.starts_with(PS_SIG)).map(|s| s.1);
    let ps = edit_ps(old_ps, l);

    let mut out = vec![0xFF, 0xD8];
    let mut xmp_done = false;
    let mut ps_done = false;
    let mut xmp_payload = XMP_SIG.to_vec();
    xmp_payload.extend_from_slice(xmp.as_bytes());
    // keep every segment in place; the new XMP and IPTC go where the old ones were, or after
    // the JFIF/EXIF headers
    let insert_new = |out: &mut Vec<u8>, xmp_done: &mut bool, ps_done: &mut bool| -> Result<()> {
        if !*xmp_done {
            out.extend(segment(0xE1, &xmp_payload)?);
            *xmp_done = true;
        }
        if !*ps_done {
            out.extend(segment(0xED, &ps)?);
            *ps_done = true;
        }
        Ok(())
    };
    let mut headers_over = false;
    for (m, p) in &segs {
        let is_header = *m == 0xE0 || (*m == 0xE1 && p.starts_with(b"Exif\0\0"));
        if !is_header && !headers_over {
            headers_over = true;
            insert_new(&mut out, &mut xmp_done, &mut ps_done)?;
        }
        if *m == 0xE1 && p.starts_with(XMP_SIG) {
            continue; // replaced
        }
        if *m == 0xED && p.starts_with(PS_SIG) {
            continue; // replaced
        }
        out.extend(segment(*m, p)?);
    }
    insert_new(&mut out, &mut xmp_done, &mut ps_done)?;
    out.extend_from_slice(&file[sos..]);
    Ok(out)
}

// ---------- on disk ----------

/// Tidy tags: trimmed, no empty ones, no repeats (ignoring capitals; the first spelling wins).
pub fn clean(tags: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    tags.iter_mut().for_each(|t| *t = t.replace(['\r', '\n'], " ").trim().to_string());
    tags.retain(|t| !t.is_empty() && seen.insert(t.to_lowercase()));
}

/// The tags and caption of a photo on disk: inside a JPEG; for other files, in the folder's
/// sidecar (`keywords=` comma-separated and `caption=`, as Picasa's `.picasa.ini` does).
pub fn of_file(folder: &Path, name: &str) -> Result<Labels> {
    let bytes = std::fs::read(folder.join(name)).with_context(|| format!("read {name}"))?;
    if segments(&bytes).is_some() {
        return Ok(read(&bytes));
    }
    let e = sidecar::read(folder).remove(name).unwrap_or_default();
    let mut tags: Vec<String> = e.get("keywords").split(',').map(String::from).collect();
    clean(&mut tags);
    Ok(Labels { tags, caption: e.get("caption").to_string() })
}

/// Changes a photo's tags and caption. A JPEG gets them written inside (the untouched original
/// is kept the first time, as with Save); other files keep them in the sidecar. Returns
/// whether anything changed.
pub fn change(folder: &Path, name: &str, f: impl FnOnce(&mut Labels)) -> Result<bool> {
    let bytes = std::fs::read(folder.join(name)).with_context(|| format!("read {name}"))?;
    if segments(&bytes).is_some() {
        let before = read(&bytes);
        let mut l = before.clone();
        f(&mut l);
        clean(&mut l.tags);
        l.caption = l.caption.trim().to_string();
        if l == before {
            return Ok(false);
        }
        crate::save::install(folder, name, &write(&bytes, &l)?)?;
        return Ok(true);
    }
    let before = of_file(folder, name)?;
    let mut l = before.clone();
    f(&mut l);
    // a comma separates tags in the sidecar line, so one inside a tag becomes a space
    l.tags.iter_mut().for_each(|t| *t = t.replace(',', " "));
    clean(&mut l.tags);
    l.caption = l.caption.trim().to_string();
    if l == before {
        return Ok(false);
    }
    sidecar::update(folder, name, |e| {
        e.set("keywords", &l.tags.join(","));
        e.set("caption", &l.caption);
    })?;
    Ok(true)
}

/// Revert in the Edit view: puts the untouched original back, then writes the photo's current
/// tags and caption into it again, so Revert undoes edits and date changes but keeps the
/// labels (as Picasa, which keeps them apart from the edits).
pub fn revert_keeping_labels(folder: &Path, name: &str) -> Result<bool> {
    let labels = of_file(folder, name)?;
    if !crate::save::revert(folder, name)? {
        return Ok(false);
    }
    change(folder, name, |l| *l = labels)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_keeps_the_original_once_and_revert_keeps_the_labels() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        let f = plain_jpeg();
        std::fs::write(dir.join("a.jpg"), &f).unwrap();
        assert!(change(dir, "a.jpg", |l| l.tags = vec![" beach ".into(), "Beach".into(), "".into(), "sea".into()]).unwrap());
        assert_eq!(of_file(dir, "a.jpg").unwrap().tags, ["beach", "sea"], "trimmed, no repeats");
        let backup = crate::save::backup_of(dir, "a.jpg");
        assert_eq!(std::fs::read(&backup).unwrap(), f, "untouched original kept");
        assert!(change(dir, "a.jpg", |l| l.caption = "Lake".into()).unwrap());
        assert_eq!(std::fs::read(&backup).unwrap(), f, "kept only the first time");
        assert!(!change(dir, "a.jpg", |l| l.caption = "Lake ".into()).unwrap(), "no change, no write");

        // an Adjust Date after tagging, then Revert: date undone, tags and caption kept
        crate::dates::adjust(dir, "a.jpg", crate::dates::Change::Set(1_000_000_000), 0).unwrap();
        assert!(crate::meta::from_bytes(&std::fs::read(dir.join("a.jpg")).unwrap()).taken.is_some(), "date written");
        assert_eq!(of_file(dir, "a.jpg").unwrap().caption, "Lake", "date change keeps the labels");
        assert!(revert_keeping_labels(dir, "a.jpg").unwrap());
        let now = std::fs::read(dir.join("a.jpg")).unwrap();
        assert_eq!(read(&now), Labels { tags: vec!["beach".into(), "sea".into()], caption: "Lake".into() });
        assert!(crate::meta::from_bytes(&now).taken.is_none(), "date change undone");
        assert_eq!(image::load_from_memory(&now).unwrap().to_rgb8(), image::load_from_memory(&f).unwrap().to_rgb8());

        // a PNG keeps them in the sidecar
        let png = dir.join("p.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([1, 2, 3])).save(&png).unwrap();
        assert!(change(dir, "p.png", |l| l.tags = vec!["a,b".into(), "c".into()]).unwrap());
        assert_eq!(sidecar::read(dir)["p.png"].get("keywords"), "a b,c");
        assert_eq!(of_file(dir, "p.png").unwrap().tags, ["a b", "c"]);
    }

    fn plain_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_fn(32, 16, |x, y| image::Rgb([(x * 8) as u8, (y * 16) as u8, 100]));
        crate::thumbs::encode_jpeg(&img, 90).unwrap()
    }
    fn picture(b: &[u8]) -> &[u8] {
        &b[segments(b).unwrap().1..]
    }

    #[test]
    fn write_then_read_and_the_picture_is_untouched() {
        let f = plain_jpeg();
        let l = Labels { tags: vec!["beach".into(), "Zürich & <friends>".into()], caption: "Sunset at the lake".into() };
        let out = write(&f, &l).unwrap();
        assert_eq!(read(&out), l);
        assert_eq!(picture(&out), picture(&f), "picture data copied unchanged");
        assert_eq!(image::load_from_memory(&out).unwrap().to_rgb8(), image::load_from_memory(&f).unwrap().to_rgb8());

        // change again: replaced, not added to
        let l2 = Labels { tags: vec!["family".into()], caption: String::new() };
        let out2 = write(&out, &l2).unwrap();
        assert_eq!(read(&out2), l2);
        assert_eq!(out2.windows(XMP_SIG.len()).filter(|w| *w == XMP_SIG).count(), 1, "one XMP block");
        assert_eq!(out2.windows(PS_SIG.len()).filter(|w| *w == PS_SIG).count(), 1, "one IPTC block");
    }

    #[test]
    fn iptc_alone_is_read_and_other_xmp_and_iptc_fields_are_kept() {
        // a file with IPTC from another program (Latin-1, a byline 2:80) and XMP holding a rating
        let f = plain_jpeg();
        let mut ps = PS_SIG.to_vec();
        let mut iptc = vec![0x1C, 2, 80, 0, 4];
        iptc.extend_from_slice(b"Anna");
        iptc.extend_from_slice(&[0x1C, 2, 25, 0, 4]);
        iptc.extend_from_slice(b"caf\xe9");
        ps.extend_from_slice(b"8BIM\x04\x04\x00\x00");
        ps.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        ps.extend_from_slice(&iptc);
        if iptc.len() & 1 == 1 {
            ps.push(0);
        }
        let xmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"4\"/></rdf:RDF></x:xmpmeta>";
        let mut xp = XMP_SIG.to_vec();
        xp.extend_from_slice(xmp.as_bytes());
        let mut g = f[..2].to_vec();
        g.extend(segment(0xE1, &xp).unwrap());
        g.extend(segment(0xED, &ps).unwrap());
        g.extend_from_slice(&f[2..]);

        assert_eq!(read(&g).tags, ["café"], "Latin-1 IPTC when XMP has none");
        let out = write(&g, &Labels { tags: vec!["paris".into()], caption: "x".into() }).unwrap();
        assert_eq!(read(&out).tags, ["paris"]);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("xmp:Rating=\"4\""), "other XMP kept");
        let recs = iptc_records(iptc_block(segments(&out).unwrap().0.iter().find(|s| s.0 == 0xED).unwrap().1).unwrap());
        assert!(recs.iter().any(|(r, d, v)| *r == 2 && *d == 80 && v == b"Anna"), "byline kept");
        let kw: Vec<_> = recs.iter().filter(|(r, d, _)| *r == 2 && *d == 25).map(|(_, _, v)| v.clone()).collect();
        assert_eq!(kw, [b"paris".to_vec()], "old keyword gone, new one alone");
        assert!(recs.iter().any(|(r, d, v)| *r == 1 && *d == 90 && v == b"\x1b%G"), "marked UTF-8");
        assert_eq!(picture(&out), picture(&g));
    }

    #[test]
    fn not_a_jpeg_is_refused() {
        assert!(write(b"\x89PNG\r\n", &Labels::default()).is_err());
        assert_eq!(read(b"\x89PNG"), Labels::default());
    }
}
