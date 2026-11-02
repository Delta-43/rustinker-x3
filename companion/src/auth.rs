use crate::{Error, Result, invalid};
use axum::http::HeaderMap;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
pub enum Auth {
    Dev { digest: String },
    Access(Access),
}
pub struct Access {
    issuer: String,
    audience: String,
    client: reqwest::Client,
    cache: Mutex<Option<(Instant, JwkSet)>>,
}
impl Auth {
    pub fn dev(bind: SocketAddr, bearer: &str) -> Result<Self> {
        if !bind.ip().is_loopback()
            || bearer.len() < 32
            || bearer.len() > 256
            || !bearer.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(invalid());
        }
        Ok(Self::Dev {
            digest: crate::hash(bearer.as_bytes()),
        })
    }
    pub fn access(team: &str, audience: &str) -> Result<Self> {
        if !team.ends_with(".cloudflareaccess.com")
            || team.len() > 253
            || !team
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
            || audience.is_empty()
            || audience.len() > 256
        {
            return Err(invalid());
        }
        Ok(Self::Access(Access {
            issuer: format!("https://{team}"),
            audience: audience.into(),
            client: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| invalid())?,
            cache: Mutex::new(None),
        }))
    }
    pub async fn check(&self, headers: &HeaderMap) -> Result<()> {
        let deny = Error(401, "unauthorised");
        match self {
            Self::Dev { digest } => {
                let value = bearer(headers).ok_or(deny)?;
                if crate::hash(value.as_bytes()) != *digest {
                    return Err(deny);
                }
                Ok(())
            }
            Self::Access(a) => {
                let jwt = unique(headers, "cf-access-jwt-assertion").ok_or(deny)?;
                if jwt.len() > 16384 {
                    return Err(deny);
                }
                let header = jsonwebtoken::decode_header(jwt).map_err(|_| deny)?;
                if header.alg != Algorithm::RS256 {
                    return Err(deny);
                }
                let kid = header.kid.ok_or(deny)?;
                let mut cache = a.cache.lock().await;
                let refresh = cache.as_ref().is_none_or(|(at, keys)| {
                    at.elapsed() > Duration::from_secs(3600)
                        || (keys.find(&kid).is_none() && at.elapsed() > Duration::from_secs(30))
                });
                if refresh {
                    let response = a
                        .client
                        .get(format!("{}/cdn-cgi/access/certs", a.issuer))
                        .send()
                        .await
                        .map_err(|_| deny)?;
                    let bytes = crate::upstream::bounded(response, 262144)
                        .await
                        .map_err(|_| deny)?;
                    let keys: JwkSet = serde_json::from_slice(&bytes).map_err(|_| deny)?;
                    *cache = Some((Instant::now(), keys));
                }
                let (_, keys) = cache.as_ref().ok_or(deny)?;
                validate(jwt, keys, &a.issuer, &a.audience)
            }
        }
    }
}
pub fn validate(jwt: &str, keys: &JwkSet, issuer: &str, audience: &str) -> Result<()> {
    let deny = Error(401, "unauthorised");
    let header = jsonwebtoken::decode_header(jwt).map_err(|_| deny)?;
    if header.alg != Algorithm::RS256 {
        return Err(deny);
    }
    let key = keys.find(&header.kid.ok_or(deny)?).ok_or(deny)?;
    let key = DecodingKey::from_jwk(key).map_err(|_| deny)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[audience]);
    validation.set_issuer(&[issuer]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.validate_nbf = true;
    validation.leeway = 30;
    jsonwebtoken::decode::<serde_json::Value>(jwt, &key, &validation).map_err(|_| deny)?;
    Ok(())
}
fn unique<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    if headers.get_all(key).iter().count() != 1 {
        return None;
    }
    headers.get(key)?.to_str().ok()
}
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    unique(headers, "authorization")?.strip_prefix("Bearer ")
}
