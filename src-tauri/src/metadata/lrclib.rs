//! LRCLIB (https://lrclib.net) — open synced-lyrics database, no auth.
//! Used as the fallback when Netease has no lyrics.

use serde_json::Value;

use super::netease::Rt;

pub struct LrcHit {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub synced: String,
    pub duration: Option<f64>,
}

pub async fn search(client: &reqwest::Client, track: &str, artist: &str) -> Rt<Vec<LrcHit>> {
    let url = reqwest::Url::parse_with_params(
        "https://lrclib.net/api/search",
        &[("track_name", track), ("artist_name", artist)],
    )?;
    let rows: Vec<Value> = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(rows.iter().filter_map(parse_hit).collect())
}

fn parse_hit(row: &Value) -> Option<LrcHit> {
    let title = row["trackName"].as_str()?.trim();
    let synced = row["syncedLyrics"].as_str()?.trim();
    if title.is_empty() || synced.is_empty() {
        return None;
    }
    Some(LrcHit {
        title: title.to_string(),
        artist: row["artistName"].as_str().unwrap_or_default().to_string(),
        album: row["albumName"].as_str().map(str::to_string),
        synced: synced.to_string(),
        duration: row["duration"].as_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_candidate_identity_and_skips_plain_lyrics() {
        let mut row = serde_json::json!({
            "trackName": "Tidal Letter",
            "artistName": "Harbor Quartet",
            "albumName": "Night Windows",
            "duration": 182.5,
            "syncedLyrics": "[00:10.00]Wait by the water"
        });
        let hit = parse_hit(&row).unwrap();
        assert_eq!(hit.title, "Tidal Letter");
        assert_eq!(hit.artist, "Harbor Quartet");
        assert_eq!(hit.album.as_deref(), Some("Night Windows"));
        assert_eq!(hit.duration, Some(182.5));
        row["syncedLyrics"] = Value::Null;
        assert!(parse_hit(&row).is_none());
    }
}
