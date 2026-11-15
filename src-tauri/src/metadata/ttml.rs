//! AMLL TTML word-level lyrics — https://github.com/amll-dev/amll-ttml-db (CC0).
//! Files are keyed by netease song id: `ncm-lyrics/{id}.ttml`.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::dto::{LyricLineDto, WordDto};

use super::netease::Rt;

pub async fn fetch(client: &reqwest::Client, netease_id: i64) -> Rt<Option<String>> {
    let url = format!(
        "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/ncm-lyrics/{netease_id}.ttml"
    );
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let text = resp.text().await?;
    if text.contains("<tt") {
        Ok(Some(text))
    } else {
        Ok(None)
    }
}

/// "HH:MM:SS.mmm" | "MM:SS.mmm" | "SS.mmm" -> seconds
fn parse_ts(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.trim().split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total)
}

fn attr_ts(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<f64> {
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.as_ref() == key {
            if let Ok(v) = a.unescape_value() {
                return parse_ts(&v);
            }
        }
    }
    None
}

/// Parse TTML into word-timed lyric lines. Lines without word spans are kept
/// as plain lines when they carry begin/end; metadata is ignored.
pub fn parse_ttml(text: &str) -> Vec<LyricLineDto> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);

    let mut lines: Vec<LyricLineDto> = Vec::new();
    let mut line_t: Option<f64> = None;
    let mut line_text = String::new();
    let mut words: Vec<WordDto> = Vec::new();
    // (begin, end, text) of the word currently being read
    let mut word: Option<(f64, Option<f64>, String)> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = e.name().local_name();
                let local = local.as_ref();
                if local == b"p" {
                    line_t = attr_ts(&e, b"begin");
                    line_text.clear();
                    words.clear();
                } else if local == b"span" && line_t.is_some() {
                    word = Some((
                        attr_ts(&e, b"begin").unwrap_or(0.0),
                        attr_ts(&e, b"end"),
                        String::new(),
                    ));
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(w) = word.as_mut() {
                    if let Ok(txt) = t.unescape() {
                        w.2.push_str(&txt);
                    }
                } else if line_t.is_some() {
                    if let Ok(txt) = t.unescape() {
                        line_text.push_str(&txt);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = e.name().local_name();
                let local = local.as_ref();
                if local == b"span" {
                    if let Some((t0, end, txt)) = word.take() {
                        if !txt.trim().is_empty() {
                            let d = end.map(|e2| (e2 - t0).max(0.0)).unwrap_or(0.3);
                            words.push(WordDto { t: t0, d, text: txt });
                        }
                    }
                } else if local == b"p" {
                    if let Some(t0) = line_t.take() {
                        let has_words = !words.is_empty();
                        let has_text = !line_text.trim().is_empty();
                        if has_words {
                            let joined = words.iter().map(|w| w.text.as_str()).collect::<String>();
                            lines.push(LyricLineDto {
                                t: t0,
                                text: joined,
                                words: Some(std::mem::take(&mut words)),
                            });
                        } else if has_text {
                            lines.push(LyricLineDto {
                                t: t0,
                                text: line_text.trim().to_string(),
                                words: None,
                            });
                            line_text.clear();
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    lines
}
