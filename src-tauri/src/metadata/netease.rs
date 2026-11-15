//! Netease Cloud Music metadata (search / lyric / cover). Anonymous endpoints
//! only; no audio streams are ever requested.

use serde_json::Value;

pub type Rt<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct NcSong {
    pub id: i64,
    pub name: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    /// milliseconds
    pub duration_ms: f64,
}

pub async fn search(client: &reqwest::Client, query: &str) -> Rt<Vec<NcSong>> {
    let url = reqwest::Url::parse_with_params(
        "https://music.163.com/api/search/get/web",
        &[("s", query), ("type", "1"), ("limit", "30"), ("offset", "0")],
    )?;
    let v: Value = client
        .get(url)
        .header("Referer", "https://music.163.com")
        .send()
        .await?
        .json()
        .await?;
    let mut songs = Vec::new();
    if let Some(arr) = v.pointer("/result/songs").and_then(|x| x.as_array()) {
        for s in arr {
            let id = s["id"].as_i64().unwrap_or(0);
            let name = s["name"].as_str().unwrap_or("").to_string();
            if id == 0 || name.is_empty() {
                continue;
            }
            let artists: Vec<String> = s["artists"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str().map(|n| n.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let album = s["album"]["name"]
                .as_str()
                .filter(|a| !a.is_empty())
                .map(|a| a.to_string());
            let duration_ms = s["duration"].as_f64().unwrap_or(0.0);
            songs.push(NcSong {
                id,
                name,
                artists,
                album,
                duration_ms,
            });
        }
    }
    Ok(songs)
}

pub async fn lyric(client: &reqwest::Client, id: i64) -> Rt<Option<String>> {
    let id_s = id.to_string();
    let url = reqwest::Url::parse_with_params(
        "https://music.163.com/api/song/lyric",
        &[("id", id_s.as_str()), ("lv", "1"), ("kv", "1"), ("tv", "-1")],
    )?;
    let v: Value = client
        .get(url)
        .header("Referer", "https://music.163.com")
        .send()
        .await?
        .json()
        .await?;
    let lrc = v
        .pointer("/lrc/lyric")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if lrc.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(lrc.to_string()))
    }
}

/// Cover URL via song/detail — the search endpoint no longer returns picUrl.
pub async fn song_detail_cover(client: &reqwest::Client, id: i64) -> Rt<Option<String>> {
    let id_s = id.to_string();
    let ids_s = format!("[{id}]");
    let url = reqwest::Url::parse_with_params(
        "https://music.163.com/api/song/detail",
        &[("id", id_s.as_str()), ("ids", ids_s.as_str())],
    )?;
    let v: Value = client
        .get(url)
        .header("Referer", "https://music.163.com")
        .send()
        .await?
        .json()
        .await?;
    Ok(v
        .pointer("/songs/0/album/picUrl")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string()))
}
