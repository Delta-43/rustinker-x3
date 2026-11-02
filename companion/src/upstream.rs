use crate::{Error, Result, invalid};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSpec {
    pub scryfall_id: String,
    pub face: u8,
    pub fit: String,
    pub orientation: String,
}
impl TokenSpec {
    pub fn validate(&self) -> Result<()> {
        uuid::Uuid::parse_str(&self.scryfall_id).map_err(|_| invalid())?;
        // Whole card only: Scryfall forbids cropping off the artist and
        // copyright line, which "cover" can do.
        if self.face > 1
            || self.fit != "contain"
            || !matches!(self.orientation.as_str(), "auto" | "landscape" | "portrait")
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn convert(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.validate()?;
        x3_tools::token(
            bytes,
            if self.fit == "cover" {
                x3_tools::Fit::Cover
            } else {
                x3_tools::Fit::Contain
            },
            match self.orientation.as_str() {
                "portrait" => x3_tools::Orientation::Portrait,
                "landscape" => x3_tools::Orientation::Landscape,
                _ => x3_tools::Orientation::Auto,
            },
        )
        .map_err(|_| Error(422, "conversion_failed"))
    }
}
pub async fn bounded(mut response: reqwest::Response, cap: usize) -> Result<Vec<u8>> {
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > cap as u64)
    {
        return Err(Error(502, "upstream_failed"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error(502, "upstream_failed"))?
    {
        if bytes.len() + chunk.len() > cap {
            return Err(Error(502, "upstream_failed"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
/// Scryfall asks clients to keep what they download for at least a day.
const CACHE_FOR: Duration = Duration::from_secs(24 * 3600);
/// Scryfall's hard limits (docs/api/rate-limits, checked 2026-11-02):
/// `/cards/search` 2 per second, other API calls 10 per second. The image
/// host has no limit; it shares the 100 ms gap anyway. After a 429, access is
/// limited for 30 s, so the companion waits at least that long.
pub fn gap(url: &reqwest::Url) -> Duration {
    if url.host_str() == Some("api.scryfall.com") && url.path().starts_with("/cards/search") {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(100)
    }
}
struct Cached {
    at: Instant,
    bytes: Vec<u8>,
}
pub struct Scryfall {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, Cached>>,
    next: Mutex<Instant>,
}
impl Scryfall {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .user_agent("X3Companion/0.1.0 (personal e-ink token display)")
                .build()
                .map_err(|_| invalid())?,
            cache: Mutex::new(HashMap::new()),
            next: Mutex::new(Instant::now()),
        })
    }
    pub fn allowed(url: &reqwest::Url) -> bool {
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some("api.scryfall.com" | "cards.scryfall.io")
            )
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
    }
    async fn fetch(&self, url: reqwest::Url, cap: usize) -> Result<Vec<u8>> {
        if !Self::allowed(&url) {
            return Err(Error(502, "upstream_failed"));
        }
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, v| v.at.elapsed() < CACHE_FOR);
            if let Some(v) = cache.get(url.as_str()) {
                return Ok(v.bytes.clone());
            }
        }
        let mut next = self.next.lock().await;
        tokio::time::sleep_until((*next).into()).await;
        *next = Instant::now() + gap(&url);
        let response = self
            .client
            .get(url.clone())
            .header(
                "Accept",
                if url.host_str() == Some("api.scryfall.com") {
                    "application/json"
                } else {
                    "image/png,image/jpeg"
                },
            )
            .send()
            .await
            .map_err(|_| Error(502, "upstream_failed"))?;
        if matches!(response.status().as_u16(), 429 | 503) {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(30)
                .clamp(30, 120);
            *next = Instant::now() + Duration::from_secs(delay);
            return Err(Error(502, "upstream_failed"));
        }
        let bytes = bounded(response, cap).await?;
        drop(next);
        let mut cache = self.cache.lock().await;
        while cache.len() >= 64
            || cache.values().map(|v| v.bytes.len()).sum::<usize>() + bytes.len() > 64 * 1024 * 1024
        {
            let Some(key) = cache
                .iter()
                .min_by_key(|(_, v)| v.at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            cache.remove(&key);
        }
        cache.insert(
            url.to_string(),
            Cached {
                at: Instant::now(),
                bytes: bytes.clone(),
            },
        );
        Ok(bytes)
    }
    pub async fn search(&self, q: &str) -> Result<serde_json::Value> {
        if q.is_empty() || q.len() > 256 || q.chars().any(char::is_control) {
            return Err(invalid());
        }
        let mut url =
            reqwest::Url::parse("https://api.scryfall.com/cards/search").map_err(|_| invalid())?;
        url.query_pairs_mut().append_pair("q", q);
        serde_json::from_slice(&self.fetch(url, 2 * 1024 * 1024).await?)
            .map_err(|_| Error(502, "upstream_failed"))
    }
    pub async fn image(&self, spec: &TokenSpec) -> Result<Vec<u8>> {
        spec.validate()?;
        let id = uuid::Uuid::parse_str(&spec.scryfall_id).map_err(|_| invalid())?;
        let url = reqwest::Url::parse(&format!("https://api.scryfall.com/cards/{id}"))
            .map_err(|_| invalid())?;
        let card: serde_json::Value =
            serde_json::from_slice(&self.fetch(url, 2 * 1024 * 1024).await?)
                .map_err(|_| Error(502, "upstream_failed"))?;
        let images = if card.get("image_uris").is_some() && spec.face == 0 {
            &card["image_uris"]
        } else {
            &card["card_faces"][spec.face as usize]["image_uris"]
        };
        let image = images["png"]
            .as_str()
            .or_else(|| images["normal"].as_str())
            .ok_or(Error(422, "conversion_failed"))?;
        let url = reqwest::Url::parse(image).map_err(|_| Error(502, "upstream_failed"))?;
        if url.host_str() != Some("cards.scryfall.io") {
            return Err(Error(502, "upstream_failed"));
        }
        self.fetch(url, 8 * 1024 * 1024).await
    }
}

pub trait Source: Send + Sync {
    fn image<'a>(
        &'a self,
        spec: &'a TokenSpec,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>> + Send + 'a>>;
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>>;
}
impl Source for Scryfall {
    fn image<'a>(
        &'a self,
        spec: &'a TokenSpec,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        Box::pin(Scryfall::image(self, spec))
    }
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>>
    {
        Box::pin(Scryfall::search(self, q))
    }
}
