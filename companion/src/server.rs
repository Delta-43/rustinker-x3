use crate::{
    Error, Result,
    auth::Auth,
    db::{Database, Submission},
    hash, invalid,
    link::{self, Fetcher, Links},
    now,
    upstream::{Scryfall, Source, TokenSpec},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, RawQuery, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
#[derive(Clone)]
pub struct App {
    pub db: Arc<Database>,
    pub auth: Arc<Auth>,
    pub source: Arc<dyn Source>,
    pub links: Arc<dyn Links>,
    pub conversions: Arc<Semaphore>,
    /// Optional private folder for fonts that may not be redistributed.
    pub fonts: Option<std::path::PathBuf>,
    /// Nonces seen on the signed device listener, newest last (ADR 026).
    pub nonces: Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
}
/// Nonces remembered. A replayed request older than this is possible only after
/// that many newer requests, or a restart; every route is idempotent.
const NONCE_MEMORY: usize = 1024;
impl App {
    pub fn new(db: Arc<Database>, auth: Auth) -> Result<Self> {
        Ok(Self {
            db,
            auth: Arc::new(auth),
            source: Arc::new(Scryfall::new()?),
            links: Arc::new(Fetcher::new()?),
            conversions: Arc::new(Semaphore::new(2)),
            fonts: None,
            nonces: Arc::default(),
        })
    }
    /// Converts an uploaded picture (ADR 028) with the token converter.
    pub async fn picture(&self, spec: ImageSpec, bytes: axum::body::Bytes) -> Result<Vec<u8>> {
        let permit = tokio::time::timeout(
            Duration::from_millis(250),
            self.conversions.clone().acquire_owned(),
        )
        .await
        .map_err(|_| Error(503, "busy"))?
        .map_err(|_| Error(503, "busy"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            spec.convert(&bytes)
        })
        .await
        .map_err(|_| Error(422, "conversion_failed"))?
    }
    pub async fn token(&self, spec: TokenSpec) -> Result<Vec<u8>> {
        let permit = tokio::time::timeout(
            Duration::from_millis(250),
            self.conversions.clone().acquire_owned(),
        )
        .await
        .map_err(|_| Error(503, "busy"))?
        .map_err(|_| Error(503, "busy"))?;
        let image = self.source.image(&spec).await?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            spec.convert(&image)
        })
        .await
        .map_err(|_| Error(422, "conversion_failed"))?
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleRequest {
    pub submission_id: String,
    pub kind: String,
    pub title: String,
    pub source_url: String,
    pub text: String,
    #[serde(default)]
    pub item: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRequest {
    pub submission_id: String,
    pub scryfall_id: String,
    pub face: u8,
    pub fit: String,
    pub orientation: String,
    pub show: bool,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    /// Card name shown in the inbox; not used for conversion.
    #[serde(default)]
    pub name: Option<String>,
}
/// An article the companion reads from a web page (ADR 015).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRequest {
    pub submission_id: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub item: Option<String>,
}
impl TokenRequest {
    pub fn spec(&self) -> TokenSpec {
        TokenSpec {
            scryfall_id: self.scryfall_id.clone(),
            face: self.face,
            fit: self.fit.clone(),
            orientation: self.orientation.clone(),
        }
    }
}
fn json<T: serde::de::DeserializeOwned>(
    input: std::result::Result<Json<T>, axum::extract::rejection::JsonRejection>,
) -> Result<T> {
    input.map(|Json(v)| v).map_err(|e| match e.status() {
        StatusCode::PAYLOAD_TOO_LARGE => Error(413, "too_large"),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => Error(415, "unsupported_media_type"),
        _ => invalid(),
    })
}
fn canonical<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| invalid())
}
fn submitted(j: crate::db::Job, new: bool) -> Response {
    (
        if new {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        },
        Json(serde_json::json!({"job":j.job,"item":j.item,"state":j.state})),
    )
        .into_response()
}
pub fn public(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route(
            "/app.css",
            get(|| async { css(include_str!("../web/app.css")) }),
        )
        .route(
            "/tokens.css",
            get(|| async { css(include_str!("../web/tokens.css")) }),
        )
        .route("/fonts/{file}", get(font))
        .route("/api/v1/articles", post(article))
        .route("/api/v1/articles/link", post(article_link))
        .route(
            "/api/v1/images",
            post(image).layer(DefaultBodyLimit::max(IMAGE_CAP)),
        )
        .route(
            "/api/v1/images/preview.png",
            post(image_preview).layer(DefaultBodyLimit::max(IMAGE_CAP)),
        )
        .route("/api/v1/tokens", post(token))
        .route("/api/v1/tokens/preview.png", get(preview))
        .route("/api/v1/scryfall/search", get(search))
        .route("/api/v1/jobs", get(jobs))
        .route("/api/v1/jobs/{job}", get(job))
        .route("/api/v1/jobs/{job}/preview.png", get(job_preview))
        .route("/api/v1/jobs/{job}/cancel", post(cancel))
        .route("/api/v1/items/{item}", delete(remove))
        .route("/api/v1/saved", get(saved))
        .route("/api/v1/saved/{id}", delete(forget))
        .route("/api/v1/saved/{id}/preview.png", get(saved_preview))
        .route("/api/v1/saved/{id}/send", post(send_again))
        .route("/api/v1/device", get(device_info))
        .fallback(|| async { Error(404, "not_found") })
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), public_auth))
        .with_state(app)
}
pub fn device(app: App) -> Router {
    Router::new()
        .route("/device/v1/hello", post(hello))
        .route("/device/v1/jobs", get(device_jobs))
        .route("/device/v1/jobs/{job}/payload", get(payload))
        .route("/device/v1/jobs/{job}/ack", post(ack))
        .fallback(|| async { device_error(Error(404, "not_found")) })
        .layer(DefaultBodyLimit::max(193))
        .layer(middleware::from_fn_with_state(app.clone(), device_auth))
        .with_state(app)
}
/// The device router for the internet-facing proxy: signed requests only, and
/// every reply carries an `X3-Mac` (ADR 026). Same routes as `device`.
pub fn device_signed(app: App) -> Router {
    Router::new()
        .route("/device/v1/hello", post(hello))
        .route("/device/v1/jobs", get(device_jobs))
        .route("/device/v1/jobs/{job}/payload", get(payload))
        .route("/device/v1/jobs/{job}/ack", post(ack))
        .fallback(|| async { device_error(Error(404, "not_found")) })
        .layer(DefaultBodyLimit::max(193))
        .layer(middleware::from_fn_with_state(app.clone(), signed_auth))
        .with_state(app)
}
fn unauthorised() -> Response {
    device_error(Error(401, "unauthorised"))
}
async fn signed_auth(State(app): State<App>, req: Request, next: Next) -> Response {
    use x3_proto::sign;
    // Bound everything read before the MAC is known to be good.
    let (parts, body) = req.into_parts();
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(nonce), Some(auth)) = (header("x3-nonce"), header("authorization")) else {
        return unauthorised();
    };
    let Some(mac_hex) = auth.strip_prefix("X3-HMAC ") else {
        return unauthorised();
    };
    let (mut mac, mut raw_nonce) = ([0u8; sign::MAC_LEN], [0u8; sign::NONCE_LEN]);
    if parts.uri.query().is_some()
        || sign::decode_hex(mac_hex, &mut mac).is_none()
        || sign::decode_hex(nonce, &mut raw_nonce).is_none()
    {
        return unauthorised();
    }
    let Ok(Some(key)) = app.db.device_key() else {
        return unauthorised();
    };
    let Ok(body) = axum::body::to_bytes(body, 193).await else {
        return unauthorised();
    };
    let want = sign::request_mac(&key, parts.method.as_str(), parts.uri.path(), nonce, &body);
    if !sign::same(&mac, &want) {
        return unauthorised();
    }
    {
        // The MAC is good, so the nonce is the device's. Refuse a repeat.
        let Ok(mut seen) = app.nonces.lock() else {
            return unauthorised();
        };
        if seen.iter().any(|n| n == nonce) {
            return unauthorised();
        }
        if seen.len() >= NONCE_MEMORY {
            seen.pop_front();
        }
        seen.push_back(nonce.to_owned());
    }
    let nonce = nonce.to_owned();
    let response = next
        .run(Request::from_parts(parts, axum::body::Body::from(body)))
        .await;
    // Sign what the router said, whatever it was (a 404 or 409 as well).
    let (mut head, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 512 * 1024).await else {
        return device_error(Error(503, "unavailable"));
    };
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let mac = sign::response_mac(&key, &nonce, head.status.as_u16(), &digest);
    let mut mac_hex = [0u8; sign::MAC_HEX];
    sign::hex(&mac, &mut mac_hex);
    if let Ok(v) = axum::http::HeaderValue::from_bytes(&mac_hex) {
        head.headers.insert("x3-mac", v);
    }
    Response::from_parts(head, axum::body::Body::from(bytes))
}
async fn public_auth(State(app): State<App>, req: Request, next: Next) -> Response {
    if req.uri().path().starts_with("/device/") {
        return Error(404, "not_found").into_response();
    }
    if req.method() != axum::http::Method::GET
        && let Some(origin) = req.headers().get("origin")
    {
        let matches = origin
            .to_str()
            .ok()
            .and_then(|s| reqwest::Url::parse(s).ok())
            .and_then(|u| u.host_str().map(str::to_owned))
            .zip(
                req.headers()
                    .get("host")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.parse::<axum::http::uri::Authority>().ok())
                    .map(|h| h.host().to_owned()),
            )
            .is_some_and(|(origin, host)| origin == host);
        if !matches {
            return Error(401, "unauthorised").into_response();
        }
    }
    if let Err(e) = app.auth.check(req.headers()).await {
        return e.into_response();
    }
    let mut response = next.run(req).await;
    if !response.headers().contains_key("cache-control") {
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
    }
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response.headers_mut().insert("content-security-policy","default-src 'self'; img-src 'self' blob: https://cards.scryfall.io; script-src 'self'; style-src 'self'; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
    response
}
async fn device_auth(State(app): State<App>, req: Request, next: Next) -> Response {
    let valid = crate::auth::bearer(req.headers())
        .map(|b| app.db.device_auth(b))
        .transpose();
    match valid {
        Ok(Some(true)) => next.run(req).await,
        _ => device_error(Error(401, "unauthorised")),
    }
}
async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../web/index.html"))
}
fn css(body: &'static str) -> Response {
    ([("content-type", "text/css; charset=utf-8")], body).into_response()
}
/// Literata ships with the companion (OFL). General Sans is read from the
/// optional private font folder only, because its licence forbids
/// redistribution. Unknown names are 404.
async fn font(State(app): State<App>, Path(file): Path<String>) -> Response {
    const CACHE: &str = "private, max-age=31536000, immutable";
    let embedded: Option<&'static [u8]> = match file.as_str() {
        "literata-latin-wght-normal.woff2" => Some(include_bytes!(
            "../web/fonts/literata-latin-wght-normal.woff2"
        )),
        "literata-latin-ext-wght-normal.woff2" => Some(include_bytes!(
            "../web/fonts/literata-latin-ext-wght-normal.woff2"
        )),
        _ => None,
    };
    let bytes = match (embedded, file.as_str(), &app.fonts) {
        (Some(b), _, _) => b.to_vec(),
        (None, "GeneralSans-Variable.woff2", Some(dir)) => {
            match tokio::fs::read(dir.join("GeneralSans-Variable.woff2")).await {
                Ok(b) if b.len() <= 1024 * 1024 => b,
                _ => return Error(404, "not_found").into_response(),
            }
        }
        _ => return Error(404, "not_found").into_response(),
    };
    (
        [("content-type", "font/woff2"), ("cache-control", CACHE)],
        bytes,
    )
        .into_response()
}
async fn script() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}
async fn article(
    State(app): State<App>,
    body: std::result::Result<Json<ArticleRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    let mut r = json(body)?;
    r.submission_id = uuid::Uuid::parse_str(&r.submission_id)
        .map_err(|_| invalid())?
        .to_string();
    if !matches!(r.kind.as_str(), "article" | "summary") {
        return Err(invalid());
    }
    if let Some(item) = &r.item {
        x3_proto::item_id(item).map_err(|_| invalid())?;
    }
    // Reject invalid limits before persisting, without retaining a second payload copy.
    x3_tools::article(
        &r.title,
        &r.source_url,
        &r.text,
        r.kind == "summary",
        now() as u64,
    )
    .map_err(|_| invalid())?;
    let c = canonical(&r)?;
    let (j, new) = app.db.submit(Submission {
        id: &r.submission_id,
        canonical: &c,
        kind: "article",
        item: r.item.as_deref(),
        show: false,
        prepared: None,
        title: Some(&r.title),
    })?;
    Ok(submitted(j, new))
}
async fn article_link(
    State(app): State<App>,
    body: std::result::Result<Json<LinkRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    let mut r = json(body)?;
    r.submission_id = uuid::Uuid::parse_str(&r.submission_id)
        .map_err(|_| invalid())?
        .to_string();
    if !matches!(r.kind.as_str(), "article" | "summary") {
        return Err(invalid());
    }
    if let Some(item) = &r.item {
        x3_proto::item_id(item).map_err(|_| invalid())?;
    }
    let url = reqwest::Url::parse(r.url.trim()).map_err(|_| invalid())?;
    if !link::allowed(&url) || !x3_proto::payload::valid_url(url.as_str()) {
        return Err(invalid());
    }
    r.url = url.to_string();
    let c = canonical(&r)?;
    let (j, new) = app.db.submit(Submission {
        id: &r.submission_id,
        canonical: &c,
        kind: "article",
        item: r.item.as_deref(),
        show: false,
        prepared: None,
        title: Some(&r.url),
    })?;
    Ok(submitted(j, new))
}
async fn token(
    State(app): State<App>,
    body: std::result::Result<Json<TokenRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    let mut r = json(body)?;
    r.submission_id = uuid::Uuid::parse_str(&r.submission_id)
        .map_err(|_| invalid())?
        .to_string();
    r.scryfall_id = uuid::Uuid::parse_str(&r.scryfall_id)
        .map_err(|_| invalid())?
        .to_string();
    r.spec().validate()?;
    if let Some(h) = &r.expected_sha256 {
        x3_proto::hash(h).map_err(|_| invalid())?;
    }
    if let Some(n) = &r.name
        && (n.is_empty() || n.len() > 200 || n.chars().any(char::is_control))
    {
        return Err(invalid());
    }
    let c = canonical(&r)?;
    if let Some(j) = app.db.repeat(&r.submission_id, &c)? {
        return Ok(submitted(j, false));
    }
    let prepared = if let Some(expected) = &r.expected_sha256 {
        let bytes = app.token(r.spec()).await?;
        if &hash(&bytes) != expected {
            return Err(Error(409, "conflict"));
        }
        Some(bytes)
    } else {
        None
    };
    let (j, new) = app.db.submit(Submission {
        id: &r.submission_id,
        canonical: &c,
        kind: "token",
        item: None,
        show: r.show,
        prepared: prepared.as_deref(),
        title: r.name.as_deref(),
    })?;
    Ok(submitted(j, new))
}
/// Largest picture upload. The web page shrinks photos to 2048 px first;
/// the converter also refuses more than 4096 px a side or 64 MiB decoded.
const IMAGE_CAP: usize = 8 * 1024 * 1024;

/// How an uploaded picture is fitted (ADR 028). The bytes travel as the
/// request body; these settings in the query.
#[derive(Clone, Debug, Serialize)]
pub struct ImageSpec {
    pub fit: String,
    pub orientation: String,
}
impl ImageSpec {
    fn validate(&self) -> Result<()> {
        if !matches!(self.fit.as_str(), "contain" | "cover")
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
/// The request as stored for repeat checks: settings and the upload's hash.
#[derive(Serialize)]
struct ImageCanonical<'a> {
    submission_id: &'a str,
    fit: &'a str,
    orientation: &'a str,
    show: bool,
    name: Option<&'a str>,
    source_sha256: String,
}
/// Only picture types: an HTML form cannot send these, so a cross-site page
/// cannot post an upload (the JSON routes rely on their content type too).
fn picture_body(headers: &axum::http::HeaderMap, body: &[u8]) -> Result<()> {
    let kind = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !matches!(
        kind,
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Err(Error(415, "unsupported_media_type"));
    }
    if body.is_empty() {
        return Err(invalid());
    }
    Ok(())
}
fn image_spec(q: &mut BTreeMap<String, String>) -> Result<ImageSpec> {
    let s = ImageSpec {
        fit: q.remove("fit").ok_or_else(invalid)?,
        orientation: q.remove("orientation").ok_or_else(invalid)?,
    };
    s.validate()?;
    Ok(s)
}
/// `POST /api/v1/images/preview.png?fit=&orientation=`, body: the picture.
async fn image_preview(
    State(app): State<App>,
    RawQuery(raw): RawQuery,
    headers: axum::http::HeaderMap,
    body: std::result::Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Result<Response> {
    let body = body.map_err(|_| Error(413, "too_large"))?;
    picture_body(&headers, &body)?;
    let mut q = query(raw, &["fit", "orientation"])?;
    let b = app.picture(image_spec(&mut q)?, body).await?;
    let h = hash(&b);
    let (out, _) = png(&b, 0)?;
    Ok((
        [
            ("content-type", "image/png"),
            ("x3-payload-sha256", h.as_str()),
        ],
        out,
    )
        .into_response())
}
/// `POST /api/v1/images?submission_id=&fit=&orientation=&show=&name=&expected_sha256=`,
/// body: the picture. Converted now; the job holds the finished payload.
async fn image(
    State(app): State<App>,
    RawQuery(raw): RawQuery,
    headers: axum::http::HeaderMap,
    body: std::result::Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Result<Response> {
    let body = body.map_err(|_| Error(413, "too_large"))?;
    picture_body(&headers, &body)?;
    let mut q = query(
        raw,
        &[
            "submission_id",
            "fit",
            "orientation",
            "show",
            "name",
            "expected_sha256",
        ],
    )?;
    let submission_id = uuid::Uuid::parse_str(&q.remove("submission_id").ok_or_else(invalid)?)
        .map_err(|_| invalid())?
        .to_string();
    let spec = image_spec(&mut q)?;
    let show = match q.remove("show").as_deref() {
        None | Some("0") => false,
        Some("1") => true,
        _ => return Err(invalid()),
    };
    let name = q.remove("name");
    if let Some(n) = &name
        && (n.is_empty() || n.len() > 200 || n.chars().any(char::is_control))
    {
        return Err(invalid());
    }
    let expected = q.remove("expected_sha256");
    if let Some(h) = &expected {
        x3_proto::hash(h).map_err(|_| invalid())?;
    }
    let c = canonical(&ImageCanonical {
        submission_id: &submission_id,
        fit: &spec.fit,
        orientation: &spec.orientation,
        show,
        name: name.as_deref(),
        source_sha256: hash(&body),
    })?;
    if let Some(j) = app.db.repeat(&submission_id, &c)? {
        return Ok(submitted(j, false));
    }
    let bytes = app.picture(spec, body).await?;
    if expected.as_ref().is_some_and(|e| *e != hash(&bytes)) {
        return Err(Error(409, "conflict"));
    }
    let (j, new) = app.db.submit(Submission {
        id: &submission_id,
        canonical: &c,
        kind: "image",
        item: None,
        show,
        prepared: Some(&bytes),
        title: name.as_deref(),
    })?;
    Ok(submitted(j, new))
}
fn query(raw: Option<String>, keys: &[&str]) -> Result<BTreeMap<String, String>> {
    let raw = raw.unwrap_or_default();
    if raw.len() > 4096 {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(&format!("http://localhost/?{raw}")).map_err(|_| invalid())?;
    let mut map = BTreeMap::new();
    for (k, v) in url.query_pairs() {
        if !keys.contains(&k.as_ref()) || map.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err(invalid());
        }
    }
    Ok(map)
}
fn spec_query(raw: Option<String>) -> Result<TokenSpec> {
    let mut q = query(raw, &["scryfall_id", "face", "fit", "orientation"])?;
    let s = TokenSpec {
        scryfall_id: q.remove("scryfall_id").ok_or_else(invalid)?,
        face: q
            .remove("face")
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?,
        fit: q.remove("fit").ok_or_else(invalid)?,
        orientation: q.remove("orientation").ok_or_else(invalid)?,
    };
    s.validate()?;
    Ok(s)
}
/// Renders one screen; returns the PNG and the number of screens.
fn png(bytes: &[u8], page: usize) -> Result<(Vec<u8>, usize)> {
    let failed = || Error(422, "conversion_failed");
    let (image, pages) = if bytes.starts_with(b"X3TK") {
        if page != 0 {
            return Err(Error(404, "not_found"));
        }
        (x3_tools::token_preview(bytes).map_err(|_| failed())?, 1)
    } else {
        let article = x3_proto::payload::Article::decode(bytes).map_err(|_| failed())?;
        let starts = sim::article_pages(article.text, 5000).map_err(|_| failed())?;
        let start = *starts.get(page).ok_or(Error(404, "not_found"))?;
        let image = sim::article(article.text, start).map_err(|_| failed())?.0;
        (image, starts.len())
    };
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| failed())?;
    Ok((out.into_inner(), pages))
}
async fn preview(State(app): State<App>, RawQuery(raw): RawQuery) -> Result<Response> {
    let b = app.token(spec_query(raw)?).await?;
    let h = hash(&b);
    let (out, _) = png(&b, 0)?;
    Ok((
        [
            ("content-type", "image/png"),
            ("x3-payload-sha256", h.as_str()),
        ],
        out,
    )
        .into_response())
}
/// Most search results the page shows.
const RESULTS: usize = 30;
/// Token search: only tokens (Scryfall `t:token`), the closest names first.
async fn search(
    State(app): State<App>,
    RawQuery(raw): RawQuery,
) -> Result<Json<serde_json::Value>> {
    let mut q = query(raw, &["q"])?;
    let words = q.remove("q").ok_or_else(invalid)?;
    let words = words.trim();
    if words.is_empty() || words.len() > 200 || words.chars().any(char::is_control) {
        return Err(invalid());
    }
    let found = app.source.search(&format!("t:token ({words})")).await?;
    Ok(Json(tokens(&found, words)))
}
/// Scryfall's answer cut down to what the page shows, ranked: the exact
/// name, then names that start with the words, then names with the words as
/// a whole word, then the rest; Scryfall's name order within each group.
pub fn tokens(found: &serde_json::Value, words: &str) -> serde_json::Value {
    use serde_json::{Value, json};
    let want = words.to_lowercase();
    let rank = |name: &str| {
        let names: Vec<String> = name.split(" // ").map(str::to_lowercase).collect();
        if names.contains(&want) {
            0
        } else if names.iter().any(|n| n.starts_with(&want)) {
            1
        } else if names
            .iter()
            .any(|n| n.split(|c: char| !c.is_alphanumeric()).any(|w| w == want))
        {
            2
        } else {
            3
        }
    };
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let pt = |v: &Value| match (text(v, "power"), text(v, "toughness")) {
        (Some(p), Some(t)) => Some(format!("{p}/{t}")),
        _ => None,
    };
    let image = |v: &Value, size: &str| {
        v.get("image_uris")
            .and_then(|i| i.get(size))
            .and_then(Value::as_str)
            .filter(|u| u.starts_with("https://cards.scryfall.io/"))
            .map(str::to_owned)
    };
    let face = |f: &Value| {
        json!({
            "name": text(f, "name"),
            "type": text(f, "type_line"),
            "text": text(f, "oracle_text"),
            "pt": pt(f),
            "colors": f.get("colors"),
            "thumb": image(f, "small"),
            "image": image(f, "normal"),
        })
    };
    let mut cards: Vec<(usize, Value)> = found
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = text(c, "id")?;
            uuid::Uuid::parse_str(&id).ok()?;
            let name = text(c, "name")?;
            // A double-faced token keeps its pictures on the faces.
            let faces: Vec<Value> = match c.get("card_faces").and_then(Value::as_array) {
                Some(f) if c.get("image_uris").is_none() => f.iter().take(2).map(face).collect(),
                _ => vec![face(c)],
            };
            Some((
                rank(&name),
                json!({
                    "id": id,
                    "name": name,
                    "set": text(c, "set_name"),
                    "year": text(c, "released_at").map(|d| d.chars().take(4).collect::<String>()),
                    "faces": faces,
                }),
            ))
        })
        .collect();
    cards.sort_by_key(|(r, _)| *r);
    let total = cards.len();
    json!({
        "data": cards.into_iter().take(RESULTS).map(|(_, c)| c).collect::<Vec<_>>(),
        "total": total,
    })
}
async fn jobs(State(app): State<App>, RawQuery(raw): RawQuery) -> Result<Json<serde_json::Value>> {
    let q = query(raw, &["state", "limit", "before"])?;
    let limit = q
        .get("limit")
        .map(|v| v.parse::<usize>())
        .transpose()
        .map_err(|_| invalid())?
        .unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(invalid());
    }
    let state = q.get("state").filter(|s| !s.is_empty()).map(String::as_str);
    if state.is_some_and(|s| {
        !matches!(
            s,
            "accepted"
                | "preparing"
                | "ready"
                | "sending"
                | "delivered"
                | "displayed"
                | "failed"
                | "cancelled"
        )
    }) {
        return Err(invalid());
    }
    // A job ID (ULID): 26 characters of Crockford base 32.
    let before = q
        .get("before")
        .filter(|b| !b.is_empty())
        .map(String::as_str);
    if before.is_some_and(|b| b.len() != 26 || !b.bytes().all(|c| c.is_ascii_alphanumeric())) {
        return Err(invalid());
    }
    let (jobs, more) = app.db.list(state, limit, before)?;
    Ok(Json(serde_json::json!({"jobs":jobs,"more":more})))
}
async fn job(State(app): State<App>, Path(id): Path<String>) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"job":app.db.job(&id)?,"history":app.db.history(&id)?}),
    ))
}
/// One screen of a queued job, `?page=N` from 0; `X3-Page-Count` gives the total.
async fn job_preview(
    State(app): State<App>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<Response> {
    let page = query(raw, &["page"])?
        .get("page")
        .map(|p| p.parse::<usize>())
        .transpose()
        .map_err(|_| invalid())?
        .unwrap_or(0);
    let b = app.db.payload(&id, false)?;
    let (out, pages) = tokio::task::spawn_blocking(move || png(&b, page))
        .await
        .map_err(|_| Error(422, "conversion_failed"))??;
    Ok((
        [
            ("content-type", "image/png".to_owned()),
            ("x3-page-count", pages.to_string()),
        ],
        out,
    )
        .into_response())
}
async fn cancel(State(app): State<App>, Path(id): Path<String>) -> Result<Json<serde_json::Value>> {
    app.db.cancel(&id)?;
    Ok(Json(serde_json::json!({"state":"cancelled"})))
}
async fn remove(State(app): State<App>, Path(item): Path<String>) -> Result<Response> {
    Ok(submitted(app.db.queue_delete(&item)?, true))
}
fn saved_id(id: &str) -> Result<()> {
    x3_proto::hash(id).map(|_| ()).map_err(|_| invalid())
}
async fn saved(State(app): State<App>) -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({"saved": app.db.saved()?})))
}
async fn forget(State(app): State<App>, Path(id): Path<String>) -> Result<StatusCode> {
    saved_id(&id)?;
    app.db.forget(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn saved_preview(State(app): State<App>, Path(id): Path<String>) -> Result<Response> {
    saved_id(&id)?;
    let (_, b) = app.db.saved_payload(&id)?;
    let (out, _) = tokio::task::spawn_blocking(move || png(&b, 0))
        .await
        .map_err(|_| Error(422, "conversion_failed"))??;
    Ok((
        [
            ("content-type", "image/png"),
            ("cache-control", "private, max-age=86400, immutable"),
        ],
        out,
    )
        .into_response())
}
/// Send a saved token or picture again: a new job with the kept payload,
/// no Scryfall lookup and no conversion.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendAgain {
    pub submission_id: String,
    pub show: bool,
}
async fn send_again(
    State(app): State<App>,
    Path(id): Path<String>,
    body: std::result::Result<Json<SendAgain>, axum::extract::rejection::JsonRejection>,
) -> Result<Response> {
    saved_id(&id)?;
    let mut r = json(body)?;
    r.submission_id = uuid::Uuid::parse_str(&r.submission_id)
        .map_err(|_| invalid())?
        .to_string();
    let c = canonical(&serde_json::json!({"saved": id, "show": r.show}))?;
    if let Some(j) = app.db.repeat(&r.submission_id, &c)? {
        return Ok(submitted(j, false));
    }
    let (entry, payload) = app.db.saved_payload(&id)?;
    let (j, new) = app.db.submit(Submission {
        id: &r.submission_id,
        canonical: &c,
        kind: if entry.kind == "image" {
            "image"
        } else {
            "token"
        },
        item: None,
        show: r.show,
        prepared: Some(&payload),
        title: entry.title.as_deref(),
    })?;
    app.db.saved_used(&id)?;
    Ok(submitted(j, new))
}
async fn device_info(State(app): State<App>) -> Result<Json<serde_json::Value>> {
    Ok(Json(app.db.device()?))
}
fn plain(status: u16, b: impl Into<axum::body::Body>) -> Response {
    let mut r = Response::new(b.into());
    *r.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    r.headers_mut()
        .insert("content-type", "text/plain; charset=utf-8".parse().unwrap());
    r
}
fn device_error(e: Error) -> Response {
    let (status, code) = match e.0 {
        400 => (400, "invalid"),
        401 => (401, "unauthorised"),
        404 => (404, "not_found"),
        409 => (409, "conflict"),
        413 => (413, "too_large"),
        _ => (503, "unavailable"),
    };
    plain(status, format!("error {code}\n"))
}
async fn hello(
    State(app): State<App>,
    body: std::result::Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let result = (|| {
        let b = body.map_err(|_| Error(413, "too_large"))?;
        x3_proto::lines::Hello::parse(&b).map_err(|_| invalid())?;
        app.db
            .hello(std::str::from_utf8(&b).map_err(|_| invalid())?)?;
        Ok::<_, Error>(plain(200, format!("ok 1 time={}\n", now())))
    })();
    result.unwrap_or_else(device_error)
}
async fn device_jobs(State(app): State<App>) -> Response {
    let result = (|| {
        let (jobs, more) = app.db.device_jobs()?;
        let mut batch = x3_proto::lines::Jobs {
            jobs: [None; 16],
            more,
        };
        for (i, j) in jobs.iter().enumerate() {
            batch.jobs[i] = Some(x3_proto::lines::Job {
                id: &j.job,
                item: &j.item,
                kind: &j.kind,
                len: j.len,
                sha256: j.sha256.as_deref(),
                show: j.show,
            });
        }
        let mut out = [0; 4096];
        let n = batch
            .write(&mut out)
            .map_err(|_| Error(503, "unavailable"))?;
        Ok::<_, Error>(plain(200, out[..n].to_vec()))
    })();
    result.unwrap_or_else(device_error)
}
async fn payload(State(app): State<App>, Path(id): Path<String>) -> Response {
    match app.db.payload(&id, true) {
        Ok(bytes) => ([("content-type", "application/octet-stream")], bytes).into_response(),
        Err(e) => device_error(e),
    }
}
async fn ack(
    State(app): State<App>,
    Path(id): Path<String>,
    body: std::result::Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let result = (|| {
        let b = body.map_err(|_| Error(413, "too_large"))?;
        let ack = x3_proto::lines::Ack::parse(&b).map_err(|_| invalid())?;
        app.db.ack(&id, ack)?;
        Ok::<_, Error>(StatusCode::NO_CONTENT.into_response())
    })();
    result.unwrap_or_else(device_error)
}
/// One preparation worker; a restart retries a persisted preparation.
pub async fn prepare_one(app: &App) -> Result<bool> {
    let Some((j, c, prepared)) = app.db.pending()? else {
        return Ok(false);
    };
    let result = if let Some(b) = prepared {
        Ok(b)
    } else if j.kind == "article" {
        if let Ok(r) = serde_json::from_str::<ArticleRequest>(&c) {
            x3_tools::article(
                &r.title,
                &r.source_url,
                &crate::reader::prepare(&r.text, &r.source_url),
                r.kind == "summary",
                j.created as u64,
            )
            .map_err(|_| Error(422, "conversion_failed"))
        } else {
            let r: LinkRequest = serde_json::from_str(&c).map_err(|_| invalid())?;
            match app.links.page(&r.url).await {
                Ok(page) => {
                    app.db.set_title(&j.job, &page.title)?;
                    x3_tools::article(
                        &page.title,
                        &r.url,
                        &crate::reader::finish(page.text),
                        r.kind == "summary",
                        j.created as u64,
                    )
                    .map_err(|_| Error(422, "conversion_failed"))
                }
                Err(e) => Err(e),
            }
        }
    } else {
        let r: TokenRequest = serde_json::from_str(&c).map_err(|_| invalid())?;
        app.token(r.spec()).await
    };
    app.db.prepared(&j.job, result.as_deref().map_err(|e| *e))?;
    Ok(true)
}
pub async fn worker(app: App) {
    loop {
        match prepare_one(&app).await {
            Ok(true) => {}
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}
