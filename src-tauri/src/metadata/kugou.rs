//! KuGou mobile search — anonymous, returns cover art directly (img field).
//! Used as the cover fallback when Netease matching fails, mirroring folia's
//! Netease→QQ→KuGou provider chain (QQ is skipped: needs login/sign).

use serde_json::Value;

use super::netease::Rt;

pub struct KgSong {
    pub name: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    /// seconds
    pub duration_s: f64,
    pub img: Option<String>,
}

async fn parse_page(client: &reqwest::Client, url: reqwest::Url) -> Rt<Vec<KgSong>> {
    let v: Value = client.get(url).send().await?.json().await?;
    let mut songs = Vec::new();
    if let Some(arr) = v.pointer("/data/info").and_then(|x| x.as_array()) {
        for s in arr {
            let name = s["songname"]
                .as_str()
                .or_else(|| s["name"].as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let artists = s["singers"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str().map(|n| n.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let album = s["album_name"]
                .as_str()
                .or_else(|| s["albumname"].as_str())
                .filter(|a| !a.is_empty())
                .map(|a| a.to_string());
            let duration_s = s["duration"].as_f64().unwrap_or(0.0);
            let img = s["img"]
                .as_str()
                .or_else(|| s["imgUrl"].as_str())
                .map(|u| u.replace("{size}", "400"));
            songs.push(KgSong {
                name,
                artists,
                album,
                duration_s,
                img,
            });
        }
    }
    Ok(songs)
}

pub async fn search(client: &reqwest::Client, query: &str) -> Rt<Vec<KgSong>> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let build = |base: &str| {
        reqwest::Url::parse_with_params(
            base,
            &[
                ("format", "json"),
                ("keyword", q.as_str()),
                ("page", "1"),
                ("pagesize", "10"),
            ],
        )
    };
    // https first, plain http as fallback
    if let Ok(url) = build("https://mobilecdn.kugou.com/api/v3/search/song") {
        if let Ok(songs) = parse_page(client, url).await {
            if !songs.is_empty() {
                return Ok(songs);
            }
        }
    }
    let url = build("http://mobilecdn.kugou.com/api/v3/search/song")?;
    parse_page(client, url).await
}
