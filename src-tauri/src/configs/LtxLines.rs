#![allow(non_snake_case)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::utils::encoding::encode_cp1251;

/// Indent and key padding as written by CInifile::save_as (`%8s%-32s = %s`).
pub const KEY_INDENT: &str = "        ";
pub const KEY_WIDTH: usize = 32;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Which codec the file on disk uses. The engine's own configs (`alife.ltx`,
/// `axr_options.ltx`) are cp1251; the faction editor writes its configs from
/// Lua in UTF-8 (see plans/launcher/faction-editor-patch-fields-plan.md §1.1).
///
/// The codec only matters for the two places text crosses the byte boundary:
/// a value handed to `set_in_section` and the pairs `get_section` returns.
/// The file itself is never decoded as a whole — see `LtxLines`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LtxEncoding {
  Cp1251,
  Utf8,
}

/// Outcome of a batched edit.
#[derive(Debug, Clone, Default)]
pub struct SetManyResult {
  /// How many `(section, key, value)` triples were written.
  pub applied: usize,
  /// Sections that do not exist in this file; their edits were skipped.
  /// Sorted and deduplicated.
  pub missing_sections: Vec<String>,
}

/// One physical line: its bytes without the terminator, and the terminator it
/// had. `None` — the last line of a file that does not end with a newline.
#[derive(Debug, Clone)]
struct Line {
  bytes: Vec<u8>,
  eol: Option<&'static str>,
}

/// Line-oriented patcher for sectioned ltx files (`[section]` + `key = value`).
/// Patches only the requested keys; every other line — comments, order,
/// indentation, key alignment, its own line ending — is written back
/// byte-for-byte.
///
/// Lines are kept as raw bytes on purpose. Section headers, keys and every
/// value this patcher writes are ASCII, so matching needs no decoding, and a
/// file whose encoding is not uniform survives untouched. That is not a
/// theoretical case: the faction editor's Lua writes name-pool sections into
/// its otherwise UTF-8 config as raw cp1251 (`state_config.script`), so a
/// player who set up name pools has a mixed file, and decoding it as UTF-8
/// would either fail or replace those bytes with U+FFFD on save.
#[derive(Debug, Clone)]
pub struct LtxLines {
  path: PathBuf,
  lines: Vec<Line>,
  /// Dominant terminator: used for inserted lines and for a last line that
  /// had none. Existing lines keep their own.
  eol: &'static str,
  enc: LtxEncoding,
  /// The file started with a UTF-8 BOM and must be saved back with one.
  bom: bool,
}

impl LtxLines {
  /// Load an existing file. `Ok(None)` — file does not exist (must not be created).
  pub fn load<P: AsRef<Path>>(path: P, enc: LtxEncoding) -> Result<Option<Self>> {
    let path = path.as_ref().to_path_buf();
    if !path.exists() {
      return Ok(None);
    }

    let mut bytes = std::fs::read(&path).with_context(|| format!("Не удалось прочитать {}", path.display()))?;
    let bom = enc == LtxEncoding::Utf8 && bytes.starts_with(BOM);
    if bom {
      bytes.drain(..BOM.len());
    }
    let (lines, eol) = split_lines(&bytes);

    Ok(Some(Self { path, lines, eol, enc, bom }))
  }

  /// Write `key = value` into `section`.
  /// `false` — the section is missing from the file, nothing was changed.
  pub fn set_in_section(&mut self, section: &str, key: &str, value: &str) -> bool {
    let Some((start, end)) = self.section_range(section) else {
      return false;
    };
    let value = self.encode_value(value);
    self.set_within(start, end, key, &value);
    true
  }

  /// Apply many `(section, key, value)` triples in one go.
  ///
  /// `set_in_section` rescans the file from the top on every call, which is
  /// fine for the handful of keys in `[alife]` but not for a faction-editor
  /// config: 6943 lines × up to 320 edits. This builds the section index once
  /// and then edits in place.
  pub fn set_many(&mut self, edits: &[(String, String, String)]) -> SetManyResult {
    let mut result = SetManyResult::default();
    if edits.is_empty() {
      return result;
    }

    let index = self.section_index();

    // Group by the section's start line so each section is visited once.
    let mut by_start: HashMap<usize, (usize, Vec<(&str, &str)>)> = HashMap::new();
    let mut missing: Vec<String> = Vec::new();

    for (section, key, value) in edits {
      match index.get(&section.as_bytes().to_ascii_lowercase()) {
        Some(&(start, end)) => {
          by_start.entry(start).or_insert_with(|| (end, Vec::new())).1.push((key.as_str(), value.as_str()));
        }
        None => missing.push(section.clone()),
      }
    }

    missing.sort();
    missing.dedup();
    result.missing_sections = missing;

    // Inserting a missing key shifts every line below it, so walk the sections
    // bottom-up: ranges above the edit point stay valid.
    let mut starts: Vec<usize> = by_start.keys().copied().collect();
    starts.sort_unstable_by(|a, b| b.cmp(a));

    for start in starts {
      let (end, keys) = by_start.remove(&start).expect("start came from the map");
      let mut end = end;
      for (key, value) in keys {
        let value = self.encode_value(value);
        let inserted = self.set_within(start, end, key, &value);
        if inserted {
          end += 1;
        }
        result.applied += 1;
      }
    }

    result
  }

  /// `true` if a `[section]` header exists (even with no keys under it).
  pub fn has_section(&self, section: &str) -> bool {
    self.section_range(section).is_some()
  }

  /// Read all `key = value` pairs of a section (comments stripped, keys as
  /// written in the file). Empty — the section is missing or empty.
  pub fn get_section(&self, section: &str) -> Vec<(String, String)> {
    let Some((start, end)) = self.section_range(section) else {
      return Vec::new();
    };

    let mut out = Vec::new();
    for line in &self.lines[start..end] {
      let Some((key, value)) = split_key_value_bytes(&line.bytes) else { continue };
      out.push((self.decode(key), self.decode(value)));
    }
    out
  }

  /// Atomically save the file: the original bytes of every untouched line,
  /// each with its own line ending, plus the BOM if there was one.
  pub fn save(&self) -> Result<()> {
    let mut out: Vec<u8> = Vec::with_capacity(self.lines.iter().map(|l| l.bytes.len() + 2).sum::<usize>() + BOM.len());
    if self.bom {
      out.extend_from_slice(BOM);
    }
    for line in &self.lines {
      out.extend_from_slice(&line.bytes);
      out.extend_from_slice(line.eol.unwrap_or(self.eol).as_bytes());
    }
    crate::configs::atomic_write_bytes(&self.path, &out)
  }

  /// Bytes to write for a value handed in as text.
  fn encode_value(&self, value: &str) -> Vec<u8> {
    match self.enc {
      LtxEncoding::Utf8 => value.as_bytes().to_vec(),
      LtxEncoding::Cp1251 => encode_cp1251(value).unwrap_or_else(|e| {
        // Every caller writes ASCII tokens (numbers, faction ids, `true`); a
        // value cp1251 cannot hold is a programming error worth a loud log,
        // not a silent corruption of the whole file.
        log::warn!("LtxLines: value {:?} is not representable in cp1251, written as UTF-8: {}", value, e);
        value.as_bytes().to_vec()
      }),
    }
  }

  /// Text for bytes read from the file (only for what `get_section` returns).
  fn decode(&self, bytes: &[u8]) -> String {
    match self.enc {
      LtxEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
      LtxEncoding::Cp1251 => encoding_rs::WINDOWS_1251.decode(bytes).0.into_owned(),
    }
  }

  /// Replace (or insert) `key` inside the already-resolved `[start, end)` body.
  /// Returns `true` when a new line was inserted (the caller's `end` shifts).
  fn set_within(&mut self, start: usize, end: usize, key: &str, value: &[u8]) -> bool {
    // 1) The key already exists in the section — replace the value only.
    for i in start..end {
      let new_line = {
        let (payload, comment) = split_comment(&self.lines[i].bytes);
        let Some(eq) = payload.iter().position(|&b| b == b'=') else { continue };
        if !payload[..eq].trim_ascii().eq_ignore_ascii_case(key.as_bytes()) {
          continue;
        }

        // `head` keeps the original prefix: all padding spaces and the `=` sign.
        let head = &payload[..=eq];
        let mut new_line = Vec::with_capacity(head.len() + 1 + value.len() + 2 + comment.map_or(0, |c| c.len()));
        new_line.extend_from_slice(head);
        new_line.push(b' ');
        new_line.extend_from_slice(value);
        if let Some(comment) = comment {
          new_line.extend_from_slice(b"  ");
          new_line.extend_from_slice(comment);
        }
        new_line
      };
      self.lines[i].bytes = new_line;
      return false;
    }

    // 2) The key is missing — insert after the last non-empty line of the section.
    let mut insert_at = end;
    while insert_at > start && self.lines[insert_at - 1].bytes.trim_ascii().is_empty() {
      insert_at -= 1;
    }
    let mut bytes = format!("{}{:<width$} = ", KEY_INDENT, key, width = KEY_WIDTH).into_bytes();
    bytes.extend_from_slice(value);
    self.lines.insert(insert_at, Line { bytes, eol: Some(self.eol) });
    true
  }

  /// Line range of the section body: `[start, end)` — from the line after the
  /// header up to the next section header (or end of file).
  fn section_range(&self, section: &str) -> Option<(usize, usize)> {
    let mut start: Option<usize> = None;

    for (i, line) in self.lines.iter().enumerate() {
      let Some(name) = parse_section_header_bytes(&line.bytes) else { continue };

      if let Some(s) = start {
        return Some((s, i));
      }
      if name.eq_ignore_ascii_case(section.as_bytes()) {
        start = Some(i + 1);
      }
    }

    start.map(|s| (s, self.lines.len()))
  }

  /// `lowercased section name -> (start, end)` for every section, built in one
  /// pass. Like `section_range`, the first occurrence of a name wins.
  fn section_index(&self) -> HashMap<Vec<u8>, (usize, usize)> {
    let mut index: HashMap<Vec<u8>, (usize, usize)> = HashMap::new();
    let mut open: Option<(Vec<u8>, usize)> = None;

    for (i, line) in self.lines.iter().enumerate() {
      let Some(name) = parse_section_header_bytes(&line.bytes) else { continue };
      if let Some((prev, start)) = open.take() {
        index.entry(prev).or_insert((start, i));
      }
      open = Some((name.to_ascii_lowercase(), i + 1));
    }
    if let Some((prev, start)) = open.take() {
      index.entry(prev).or_insert((start, self.lines.len()));
    }

    index
  }
}

/// Split file bytes into lines, remembering each line's own terminator, and
/// pick the dominant terminator for lines that get inserted.
fn split_lines(bytes: &[u8]) -> (Vec<Line>, &'static str) {
  let mut lines: Vec<Line> = Vec::new();
  let (mut crlf, mut lf) = (0usize, 0usize);
  let mut start = 0usize;

  for (i, &b) in bytes.iter().enumerate() {
    if b != b'\n' {
      continue;
    }
    let (end, eol) = if i > start && bytes[i - 1] == b'\r' {
      crlf += 1;
      (i - 1, "\r\n")
    } else {
      lf += 1;
      (i, "\n")
    };
    lines.push(Line { bytes: bytes[start..end].to_vec(), eol: Some(eol) });
    start = i + 1;
  }
  if start < bytes.len() {
    lines.push(Line { bytes: bytes[start..].to_vec(), eol: None });
  }

  // Same choice the old text-based patcher made when the file had any CRLF.
  let dominant = if crlf > 0 && crlf >= lf { "\r\n" } else { "\n" };
  (lines, dominant)
}

/// `(payload, comment)` — the part before the first `;` and, when present, the
/// `;…` tail itself.
fn split_comment(line: &[u8]) -> (&[u8], Option<&[u8]>) {
  match line.iter().position(|&b| b == b';') {
    Some(pos) => (&line[..pos], Some(&line[pos..])),
    None => (line, None),
  }
}

/// `[alife]` -> `alife`; `[alife]:base1,base2` -> `alife`; otherwise `None`.
pub fn parse_section_header_bytes(line: &[u8]) -> Option<&[u8]> {
  let trimmed = line.trim_ascii();
  if !trimmed.starts_with(b"[") {
    return None;
  }
  let close = trimmed.iter().position(|&b| b == b']')?;
  Some(trimmed[1..close].trim_ascii())
}

/// `key = value` of a body line, comment stripped. `None` for headers, blank
/// lines, comment-only lines and the bare model paths the faction editor
/// writes into its `*_visuals_*` sections.
pub fn split_key_value_bytes(line: &[u8]) -> Option<(&[u8], &[u8])> {
  let (payload, _) = split_comment(line);
  let eq = payload.iter().position(|&b| b == b'=')?;
  let key = payload[..eq].trim_ascii();
  if key.is_empty() {
    return None;
  }
  Some((key, payload[eq + 1..].trim_ascii()))
}

/// Text variant of `parse_section_header_bytes` for callers holding decoded
/// text (the fragment parser, the developer-side diff). The delimiters are
/// ASCII, so the slices stay on char boundaries.
pub fn parse_section_header(line: &str) -> Option<&str> {
  parse_section_header_bytes(line.as_bytes()).and_then(|b| std::str::from_utf8(b).ok())
}

/// Text variant of `split_key_value_bytes`.
pub fn split_key_value(line: &str) -> Option<(&str, &str)> {
  let (key, value) = split_key_value_bytes(line.as_bytes())?;
  Some((std::str::from_utf8(key).ok()?, std::str::from_utf8(value).ok()?))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// UTF-8 CRLF, no indent, `=` aligned far right — the shape of
  /// `faction_editor_config.ltx`. The visuals section deliberately carries the
  /// `<path> = ` form that file uses.
  const SAMPLE_CONFIG: &str = concat!(
    "[alfa]\r\n",
    "fire_wound_immunity_leader                                            = 0.11\r\n",
    "name_rus                                                              = ЧВК_«Альфа»\r\n",
    "power                                                                 = 13\r\n",
    "\r\n",
    "[alfa_veteran]\r\n",
    "fire_wound_immunity                                                   = 0.43\r\n",
    "min_money                                                             = 1000\r\n",
    "\r\n",
    "[alfa_visuals_leader]\r\n",
    "actors\\stalker_alfa\\alfa_leader                                       = \r\n",
  );

  /// UTF-8 LF, engine indent — the shape of `faction_editor_default_config.ltx`.
  const SAMPLE_DEFAULT: &str = concat!(
    "[alfa]\n",
    "        fire_wound_immunity_leader       = 0.06\n",
    "        power                            = 13\n",
    " \n",
    "[alfa_veteran]\n",
    "        fire_wound_immunity              = 0.36\n",
  );

  fn temp_file(name: &str, content: &[u8]) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ltx_lines_{}_{}_{}.ltx", name, std::process::id(), n));
    std::fs::write(&path, content).unwrap();
    path
  }

  #[test]
  fn utf8_crlf_roundtrip_keeps_every_untouched_byte() {
    let path = temp_file("crlf", SAMPLE_CONFIG.as_bytes());
    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();

    assert!(ltx.set_in_section("alfa", "fire_wound_immunity_leader", "0.06"));
    ltx.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    // Only the one value changed; the column-71 alignment survives.
    assert!(text.contains("fire_wound_immunity_leader                                            = 0.06\r\n"));
    assert!(text.contains("name_rus                                                              = ЧВК_«Альфа»\r\n"));
    assert!(text.contains("power                                                                 = 13\r\n"));
    // The `<path> = ` line of a visuals section must survive untouched.
    assert!(text.contains("actors\\stalker_alfa\\alfa_leader                                       = \r\n"));
    assert!(!text.contains('\u{feff}'), "no BOM must be introduced");
    // Everything but the edited line is byte-identical to the input.
    assert_eq!(text.replace("= 0.06\r\n", "= 0.11\r\n"), SAMPLE_CONFIG);
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn utf8_lf_file_stays_lf() {
    let path = temp_file("lf", SAMPLE_DEFAULT.as_bytes());
    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();

    assert!(ltx.set_in_section("alfa_veteran", "fire_wound_immunity", "0.30"));
    ltx.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("\r\n"), "LF file must not gain CRLF");
    assert!(text.contains("        fire_wound_immunity              = 0.30\n"));
    assert!(text.ends_with("\n"));
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn set_many_matches_repeated_set_in_section() {
    let edits: Vec<(String, String, String)> = vec![
      ("alfa".into(), "power".into(), "20".into()),
      ("alfa_veteran".into(), "min_money".into(), "2000".into()),
      ("alfa".into(), "fire_wound_immunity_leader".into(), "0.05".into()),
      // Key absent from an existing section — must be inserted.
      ("alfa_veteran".into(), "max_money".into(), "9000".into()),
    ];

    let one_by_one = temp_file("one_by_one", SAMPLE_CONFIG.as_bytes());
    let mut a = LtxLines::load(&one_by_one, LtxEncoding::Utf8).unwrap().unwrap();
    for (section, key, value) in &edits {
      assert!(a.set_in_section(section, key, value));
    }
    a.save().unwrap();

    let batched = temp_file("batched", SAMPLE_CONFIG.as_bytes());
    let mut b = LtxLines::load(&batched, LtxEncoding::Utf8).unwrap().unwrap();
    let res = b.set_many(&edits);
    b.save().unwrap();

    assert_eq!(res.applied, 4);
    assert!(res.missing_sections.is_empty());
    assert_eq!(std::fs::read(&one_by_one).unwrap(), std::fs::read(&batched).unwrap());

    let text = std::fs::read_to_string(&batched).unwrap();
    assert!(text.contains("        max_money                        = 9000\r\n"), "inserted key uses engine padding");
    // The insert must land inside [alfa_veteran], before the next header.
    let inserted = text.find("max_money").unwrap();
    let next_header = text.find("[alfa_visuals_leader]").unwrap();
    assert!(inserted < next_header);

    std::fs::remove_file(&one_by_one).ok();
    std::fs::remove_file(&batched).ok();
  }

  #[test]
  fn set_many_reports_missing_sections_and_skips_them() {
    let path = temp_file("missing", SAMPLE_CONFIG.as_bytes());
    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();
    let before = std::fs::read(&path).unwrap();

    let res = ltx.set_many(&[
      ("faction_42".into(), "power".into(), "5".into()),
      ("faction_42_veteran".into(), "min_money".into(), "1".into()),
      ("faction_42".into(), "power_leader".into(), "7".into()),
    ]);
    ltx.save().unwrap();

    assert_eq!(res.applied, 0);
    assert_eq!(res.missing_sections, vec!["faction_42".to_string(), "faction_42_veteran".to_string()]);
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing may be written for missing sections");
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn utf8_bom_is_preserved() {
    let mut content = BOM.to_vec();
    content.extend_from_slice(SAMPLE_DEFAULT.as_bytes());
    let path = temp_file("bom", &content);

    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();
    assert!(ltx.has_section("alfa"), "BOM must not hide the first section");
    assert!(ltx.set_in_section("alfa", "power", "14"));
    ltx.save().unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], BOM, "BOM must be written back");
    assert_eq!(bytes.iter().filter(|&&b| b == 0xEF).count(), 1, "exactly one BOM");
    std::fs::remove_file(&path).ok();
  }

  /// The faction editor's Lua writes name-pool sections as raw cp1251 into an
  /// otherwise UTF-8 config. Such a file must be patchable, and the cp1251
  /// bytes must come back exactly as they were — not rejected, not turned
  /// into U+FFFD.
  #[test]
  fn mixed_encoding_lines_pass_through_untouched() {
    // cp1251 for "Сидорович"
    let cp1251_name: &[u8] = &[0xD1, 0xE8, 0xE4, 0xEE, 0xF0, 0xEE, 0xE2, 0xE8, 0xF7];
    let mut content = Vec::new();
    content.extend_from_slice(SAMPLE_DEFAULT.as_bytes());
    content.extend_from_slice(b" \n[alfa_names_default]\n        name_cnt                         = 1\n        name_0                           = ");
    content.extend_from_slice(cp1251_name);
    content.extend_from_slice(b"\n");
    let path = temp_file("mixed", &content);

    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();
    assert!(ltx.set_in_section("alfa", "power", "15"));
    ltx.save().unwrap();

    let after = std::fs::read(&path).unwrap();
    assert!(after.windows(cp1251_name.len()).any(|w| w == cp1251_name), "cp1251 bytes must survive byte-for-byte");
    assert!(!after.windows(3).any(|w| w == [0xEF, 0xBF, 0xBD]), "no U+FFFD may be introduced");
    let expected = {
      let s = String::from_utf8_lossy(&content).into_owned();
      s.replacen("        power                            = 13\n", "        power                            = 15\n", 1)
    };
    assert_eq!(String::from_utf8_lossy(&after), expected);
    std::fs::remove_file(&path).ok();
  }

  /// A file with mixed line endings keeps each line's own ending; only an
  /// inserted line takes the dominant one.
  #[test]
  fn mixed_eol_is_preserved_per_line() {
    let content = b"[alfa]\r\n        power = 13\n        power_leader = 30\r\n[alfa_veteran]\r\n        min_money = 1000\r\n";
    let path = temp_file("eol", content);

    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();
    assert!(ltx.set_in_section("alfa", "power", "15"));
    assert!(ltx.set_in_section("alfa", "max_money", "9"));
    ltx.save().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("        power = 15\n"), "the LF line keeps LF");
    assert!(text.contains("        power_leader = 30\r\n"), "the CRLF line keeps CRLF");
    assert!(text.contains("        max_money                        = 9\r\n"), "an inserted line uses the dominant CRLF");
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn file_without_final_newline_gets_one() {
    let path = temp_file("nofinal", b"[alfa]\r\n        power = 13");
    let mut ltx = LtxLines::load(&path, LtxEncoding::Utf8).unwrap().unwrap();
    assert!(ltx.set_in_section("alfa", "power", "14"));
    ltx.save().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"[alfa]\r\n        power = 14\r\n");
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn split_key_value_ignores_bare_visual_paths() {
    assert_eq!(split_key_value("        power = 13"), Some(("power", "13")));
    assert_eq!(split_key_value("        actors\\stalker_alfa\\alfa_leader"), None);
    assert_eq!(split_key_value("path = "), Some(("path", "")));
    assert_eq!(split_key_value("[alfa]"), None);
    assert_eq!(split_key_value(""), None);
    assert_eq!(split_key_value("power = 13 ; note"), Some(("power", "13")));
  }

  #[test]
  fn missing_file_is_not_created() {
    let path = std::env::temp_dir().join("ltx_lines_never_created.ltx");
    std::fs::remove_file(&path).ok();
    assert!(LtxLines::load(&path, LtxEncoding::Utf8).unwrap().is_none());
    assert!(!path.exists());
  }
}
