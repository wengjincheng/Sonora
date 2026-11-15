//! QQ Music anonymous search (client_search_cp) — highest-quality candidate
//! pool for covers: originals rank first even when Netease hides them behind
//! VIP, and album covers come straight from `albummid` (no extra request).

use serde_json::Value;

use super::netease::Rt;

pub struct QqSong {
    pub songname: String,
    pub singers: Vec<String>,
    pub albumname: Option<String>,
    pub albummid: Option<String>,
    /// needed by the lyric endpoint
    pub songmid: Option<String>,
    /// seconds
    pub interval_s: f64,
}

/// Album cover URL template (verified anonymous CDN access).
pub fn cover_url(albummid: &str) -> String {
    format!("https://y.gtimg.cn/music/photo_new/T002R800x800M000{albummid}.jpg")
}

pub async fn search(client: &reqwest::Client, query: &str) -> Rt<Vec<QqSong>> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let url = reqwest::Url::parse_with_params(
        "https://c.y.qq.com/soso/fcgi-bin/client_search_cp",
        &[("w", q.as_str()), ("format", "json"), ("n", "20"), ("p", "1"), ("cr", "1")],
    )?;
    let text = client
        .get(url)
        .header("Referer", "https://y.qq.com/")
        .send()
        .await?
        .text()
        .await?;
    // some responses come jsonp-wrapped even with format=json
    let body = text
        .trim()
        .trim_start_matches("callback(")
        .trim_end_matches(')');
    let v: Value = serde_json::from_str(body)?;
    let mut songs = Vec::new();
    if let Some(arr) = v.pointer("/data/song/list").and_then(|x| x.as_array()) {
        for s in arr {
            let songname = s["songname"].as_str().unwrap_or("").to_string();
            if songname.is_empty() {
                continue;
            }
            let singers = s["singer"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str().map(|n| n.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let albumname = s["albumname"]
                .as_str()
                .filter(|a| !a.is_empty())
                .map(|a| a.to_string());
            let albummid = s["albummid"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string());
            let songmid = s["songmid"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string());
            let interval_s = s["interval"].as_f64().unwrap_or(0.0);
            songs.push(QqSong {
                songname,
                singers,
                albumname,
                albummid,
                songmid,
                interval_s,
            });
        }
    }
    Ok(songs)
}

/// Anonymous lyric fetch. `nobase64=1` returns plain LRC text; the response is
/// JSON (sometimes jsonp-wrapped) and HTML-escapes a few characters.
pub async fn lyric(client: &reqwest::Client, songmid: &str) -> Rt<Option<String>> {
    let url = reqwest::Url::parse_with_params(
        "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg",
        &[
            ("songmid", songmid),
            ("format", "json"),
            ("nobase64", "1"),
            ("g_tk", "5381"),
            ("loginUin", "0"),
            ("hostUin", "0"),
            ("inCharset", "utf8"),
            ("outCharset", "utf-8"),
            ("notice", "0"),
            ("platform", "yqq.json"),
            ("needNewCode", "0"),
        ],
    )?;
    let text = client
        .get(url)
        .header("Referer", "https://y.qq.com/")
        .send()
        .await?
        .text()
        .await?;
    let body = text.trim();
    let body = body
        .trim_start_matches("MusicJsonCallback(")
        .trim_start_matches("callback(")
        .trim_end_matches(')');
    let v: Value = serde_json::from_str(body)?;
    if v["code"].as_i64().unwrap_or(0) != 0 || v["retcode"].as_i64().unwrap_or(0) != 0 {
        return Err(format!("qq lyric retcode={} code={}", v["retcode"], v["code"]).into());
    }
    let lrc = v["lyric"].as_str().unwrap_or("");
    if lrc.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(unescape_html(lrc)))
    }
}

fn unescape_html(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&#10;", "\n")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
