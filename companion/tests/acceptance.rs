use axum::{Router, body::Body, http::Request};
use http_body_util::BodyExt;
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;
use x3_companion::{
    Result,
    auth::Auth,
    db::Database,
    server::{self, App},
    upstream::{Source, TokenSpec},
};
const DEV: &str = "development-test-token-not-a-real-credential";
const DEVICE: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("x3-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture;
impl Source for Fixture {
    fn image<'a>(
        &'a self,
        _: &'a TokenSpec,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        Box::pin(async {
            let image = image::GrayImage::from_pixel(100, 200, image::Luma([128]));
            let mut out = std::io::Cursor::new(Vec::new());
            image.write_to(&mut out, image::ImageFormat::Png).unwrap();
            Ok(out.into_inner())
        })
    }
    fn search<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>>
    {
        Box::pin(async { Ok(serde_json::json!({"data":[]})) })
    }
}
fn app(path: &std::path::Path, quota: u64) -> App {
    let db = Arc::new(Database::open(path, quota).unwrap());
    if db.device().unwrap().is_null() {
        db.add_device("test", DEVICE).unwrap();
    }
    let mut app = App::new(db, Auth::dev("127.0.0.1:0".parse().unwrap(), DEV).unwrap()).unwrap();
    app.source = Arc::new(Fixture);
    app
}
async fn call(
    router: Router,
    method: &str,
    path: &str,
    token: &str,
    body: impl Into<Body>,
) -> (u16, axum::http::HeaderMap, Vec<u8>) {
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(body.into())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}
fn request() -> serde_json::Value {
    serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"kind":"article","title":"Zażółć gęślą jaźń","source_url":"https://example.org","text":"# Polish\n\nPchnąć w tę łódź jeża."})
}
async fn submit(app: &App, r: &serde_json::Value) -> String {
    let (s, _, b) = call(
        server::public(app.clone()),
        "POST",
        "/api/v1/articles",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
    serde_json::from_slice::<serde_json::Value>(&b).unwrap()["job"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn durable_accept_repeat_conflict_restart_backup() {
    let dir = Temp::new();
    let path = dir.0.join("inbox.sqlite");
    let a = app(&path, 4 * 1024 * 1024);
    let r = request();
    let id = submit(&a, &r).await;
    assert_eq!(a.db.job(&id).unwrap().state, "accepted");
    drop(a);
    let a = app(&path, 4 * 1024 * 1024);
    a.db.restart().unwrap();
    assert!(server::prepare_one(&a).await.unwrap());
    assert_eq!(a.db.job(&id).unwrap().state, "ready");
    let (s, _, b) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/articles",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&b).unwrap()["job"],
        id
    );
    let mut changed = r.clone();
    changed["text"] = "changed".into();
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/articles",
            DEV,
            changed.to_string()
        )
        .await
        .0,
        409
    );
    assert_eq!(a.db.list(None, 100, None).unwrap().0.len(), 1);
    let backup = dir.0.join("backup.sqlite");
    a.db.backup(&backup).unwrap();
    assert_eq!(Database::restore_check(&backup).unwrap(), 1);
    let restored = Database::open(&backup, 4 * 1024 * 1024).unwrap();
    assert_eq!(
        restored.job(&id).unwrap().sha256,
        a.db.job(&id).unwrap().sha256
    );
    assert_eq!(
        restored.payload(&id, false).unwrap(),
        a.db.payload(&id, false).unwrap()
    );
}
#[tokio::test]
async fn cancellation_three_races_and_lost_ack() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    // Cancel wins: no payload and stale skip ack gets 404.
    let id = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    let hash = a.db.job(&id).unwrap().sha256.unwrap();
    a.db.cancel(&id).unwrap();
    a.db.cancel(&id).unwrap();
    assert_eq!(a.db.payload(&id, true).unwrap_err().0, 404);
    assert!(a.db.device_jobs().unwrap().0.is_empty());
    assert_eq!(
        a.db.ack(&id, x3_proto::lines::Ack::Committed(Some(&hash)))
            .unwrap_err()
            .0,
        404
    );
    // Delivery wins: cancellation never becomes possible again, even after lease expiry.
    let id = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    let bytes = a.db.payload(&id, true).unwrap();
    let hash = x3_companion::hash(&bytes);
    assert_eq!(a.db.cancel(&id).unwrap_err().0, 409);
    a.db.lock()
        .unwrap()
        .execute("UPDATE jobs SET lease_until=0 WHERE id=?1", [&id])
        .unwrap();
    assert_eq!(a.db.device_jobs().unwrap().0.len(), 1);
    assert_eq!(a.db.cancel(&id).unwrap_err().0, 409);
    assert_eq!(
        a.db.ack(&id, x3_proto::lines::Ack::Committed(Some(&"0".repeat(64))))
            .unwrap_err()
            .0,
        409
    );
    a.db.ack(&id, x3_proto::lines::Ack::Committed(Some(&hash)))
        .unwrap();
    a.db.ack(&id, x3_proto::lines::Ack::Committed(Some(&hash)))
        .unwrap();
    assert_eq!(a.db.job(&id).unwrap().state, "delivered");
    assert_eq!(
        a.db.history(&id)
            .unwrap()
            .iter()
            .filter(|h| h["state"] == "delivered")
            .count(),
        1
    );
    let delete = a.db.queue_delete(&a.db.job(&id).unwrap().item).unwrap();
    a.db.device_jobs().unwrap();
    assert_eq!(a.db.cancel(&delete.job).unwrap_err().0, 409);
    a.db.ack(&delete.job, x3_proto::lines::Ack::Committed(None))
        .unwrap();
}
#[tokio::test]
async fn real_axum_ack_over_tcp() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let id = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    let h = a.db.job(&id).unwrap().sha256.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, server::device(a)).await.unwrap() });
    let body = format!("committed {h}\n");
    let mut head = [0; 1024];
    let n = x3_proto::http::request(
        x3_proto::config::Address {
            ip: [127, 0, 0, 1],
            port: addr.port(),
        },
        DEVICE,
        x3_proto::http::Route::Ack(&id),
        body.len(),
        &mut head,
    )
    .unwrap();
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    socket.write_all(&head[..n]).await.unwrap();
    socket.write_all(body.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    socket.read_to_end(&mut raw).await.unwrap();
    task.abort();
    assert!(
        !String::from_utf8_lossy(&raw)
            .to_lowercase()
            .contains("content-length")
    );
    let parsed = x3_proto::http::ResponseHead::parse(&raw, 0)
        .unwrap()
        .unwrap();
    assert_eq!(parsed.status, 204);
    assert_eq!(parsed.content_length, 0);
}
#[tokio::test]
async fn preview_hash_and_concurrency() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let path = "/api/v1/tokens/preview.png?scryfall_id=00000000-0000-4000-8000-000000000001&face=0&fit=contain&orientation=portrait";
    let (status, headers, b) = call(server::public(a.clone()), "GET", path, DEV, "").await;
    assert_eq!(status, 200);
    assert!(b.starts_with(b"\x89PNG"));
    assert!(a.db.list(None, 100, None).unwrap().0.is_empty());
    let hash = headers["x3-payload-sha256"].to_str().unwrap();
    let mut r = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"scryfall_id":"00000000-0000-4000-8000-000000000001","face":0,"fit":"contain","orientation":"portrait","show":true,"expected_sha256":"0".repeat(64)});
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/tokens",
            DEV,
            r.to_string()
        )
        .await
        .0,
        409
    );
    assert!(a.db.list(None, 100, None).unwrap().0.is_empty());
    r["expected_sha256"] = hash.into();
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/tokens",
            DEV,
            r.to_string()
        )
        .await
        .0,
        202
    );
    server::prepare_one(&a).await.unwrap();
    let j = a.db.list(None, 100, None).unwrap().0.remove(0);
    assert_eq!(j.sha256.as_deref(), Some(hash));
    a.db.ack(&j.job, x3_proto::lines::Ack::Committed(Some(hash)))
        .unwrap();
    a.db.ack(&j.job, x3_proto::lines::Ack::Displayed(hash))
        .unwrap();
    assert_eq!(a.db.job(&j.job).unwrap().state, "displayed");
    let _p = a.conversions.acquire_many(2).await.unwrap();
    assert_eq!(
        call(server::public(a.clone()), "GET", path, DEV, "")
            .await
            .0,
        503
    );
    assert_eq!(
        call(
            server::public(a.clone()),
            "GET",
            &(path.to_owned() + "&face=0"),
            DEV,
            ""
        )
        .await
        .0,
        400
    );
}
#[tokio::test]
async fn input_auth_and_full_storage() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/articles",
            "wrong",
            request().to_string()
        )
        .await
        .0,
        401
    );
    assert_eq!(
        call(server::device(a.clone()), "GET", "/device/v1/jobs", DEV, "")
            .await
            .0,
        401
    );
    assert_eq!(
        call(server::public(a.clone()), "GET", "/device/v1/jobs", DEV, "")
            .await
            .0,
        404
    );
    for b in [
        "{}".to_owned(),
        "{".into(),
        request()
            .to_string()
            .replacen("{", "{\"title\":\"duplicate\",", 1),
        request().to_string().replacen("{", "{\"extra\":1,", 1),
    ] {
        assert_eq!(
            call(
                server::public(a.clone()),
                "POST",
                "/api/v1/articles",
                DEV,
                b
            )
            .await
            .0,
            400
        );
    }
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/articles",
            DEV,
            "x".repeat(1024 * 1024 + 1)
        )
        .await
        .0,
        413
    );
    assert!(a.db.list(None, 100, None).unwrap().0.is_empty());
    // Real SQLite page exhaustion, not a stubbed error code.
    {
        let c = a.db.lock().unwrap();
        let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
        c.pragma_update(None, "max_page_count", pages).unwrap();
    }
    let mut r = request();
    r["text"] = "x".repeat(60000).into();
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/articles",
            DEV,
            r.to_string()
        )
        .await
        .0,
        507
    );
    assert!(a.db.list(None, 100, None).unwrap().0.is_empty());
    assert!(Auth::dev("0.0.0.0:8787".parse().unwrap(), DEV).is_err());
    assert!(Auth::access("evil.example", "aud").is_err());
}
#[test]
fn source_url_allowlist() {
    use x3_companion::upstream::Scryfall;
    for u in [
        "https://api.scryfall.com/cards/x",
        "https://cards.scryfall.io/x",
    ] {
        assert!(Scryfall::allowed(&u.parse().unwrap()));
    }
    for u in [
        "http://api.scryfall.com/a",
        "https://cards.scryfall.io.evil.com/a",
        "https://api.scryfall.com:8443/a",
        "https://user@api.scryfall.com/a",
        "https://127.0.0.1/a",
    ] {
        assert!(!Scryfall::allowed(&u.parse().unwrap()));
    }
}
#[tokio::test]
async fn public_access_rejects_forged_identity() {
    let dir = Temp::new();
    let db = Arc::new(Database::open(dir.0.join("db"), 4 * 1024 * 1024).unwrap());
    let a = App::new(
        db,
        Auth::access("example.cloudflareaccess.com", "aud").unwrap(),
    )
    .unwrap();
    let response = server::public(a)
        .oneshot(
            Request::builder()
                .uri("/api/v1/jobs")
                .header("Cf-Access-Authenticated-User-Email", "owner@example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

#[test]
fn signed_access_tokens_require_correct_claims_and_algorithm() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
    let private = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    let jwks:jsonwebtoken::jwk::JwkSet=serde_json::from_value(serde_json::json!({"keys":[{"kty":"RSA","kid":"test-key","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(private.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(private.e().to_bytes_be())}]})).unwrap();
    let key = jsonwebtoken::EncodingKey::from_rsa_der(private.to_pkcs1_der().unwrap().as_bytes());
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key".into());
    let claims = serde_json::json!({"iss":"https://example.cloudflareaccess.com","aud":["app"],"exp":x3_companion::now()+3600,"nbf":x3_companion::now()-1});
    let jwt = jsonwebtoken::encode(&header, &claims, &key).unwrap();
    assert!(
        x3_companion::auth::validate(&jwt, &jwks, "https://example.cloudflareaccess.com", "app")
            .is_ok()
    );
    for (field, value) in [
        ("iss", serde_json::json!("https://evil.example")),
        ("aud", serde_json::json!(["other"])),
        ("exp", serde_json::json!(x3_companion::now() - 3600)),
        ("nbf", serde_json::json!(x3_companion::now() + 3600)),
    ] {
        let mut bad = claims.clone();
        bad[field] = value;
        let jwt = jsonwebtoken::encode(&header, &bad, &key).unwrap();
        assert!(
            x3_companion::auth::validate(
                &jwt,
                &jwks,
                "https://example.cloudflareaccess.com",
                "app"
            )
            .is_err()
        );
    }
    let mut missing = claims.clone();
    missing.as_object_mut().unwrap().remove("exp");
    let jwt = jsonwebtoken::encode(&header, &missing, &key).unwrap();
    assert!(
        x3_companion::auth::validate(&jwt, &jwks, "https://example.cloudflareaccess.com", "app")
            .is_err()
    );
    let jwt = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(b"untrusted"),
    )
    .unwrap();
    assert!(
        x3_companion::auth::validate(&jwt, &jwks, "https://example.cloudflareaccess.com", "app")
            .is_err()
    );
}

#[tokio::test]
async fn opening_database_does_not_reset_live_worker() {
    let dir = Temp::new();
    let path = dir.0.join("db");
    let a = app(&path, 4 * 1024 * 1024);
    let id = submit(&a, &request()).await;
    a.db.pending().unwrap().unwrap();
    assert_eq!(a.db.job(&id).unwrap().state, "preparing");
    let other = Database::open(&path, 4 * 1024 * 1024).unwrap();
    assert_eq!(other.job(&id).unwrap().state, "preparing");
    other.restart().unwrap();
    assert_eq!(a.db.job(&id).unwrap().state, "accepted");
    server::prepare_one(&a).await.unwrap();
    assert_eq!(a.db.job(&id).unwrap().state, "ready");
}

#[tokio::test]
async fn process_killed_after_accept_keeps_job() {
    use std::process::{Command, Stdio};
    let dir = Temp::new();
    let path = dir.0.join("inbox.sqlite");
    let a = app(&path, 4 * 1024 * 1024);
    drop(a);
    let free_port = || {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let public = free_port();
    let mut device = free_port();
    while device == public {
        device = free_port();
    }
    let start = || {
        Command::new(env!("CARGO_BIN_EXE_x3-companion"))
            .args(["serve", "--dev-insecure"])
            .env("X3_DATA_DIR", &dir.0)
            .env("X3_PUBLIC_BIND", format!("127.0.0.1:{public}"))
            .env("X3_DEVICE_BIND", format!("127.0.0.1:{device}"))
            .env("X3_DEV_TOKEN", DEV)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(start());
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{public}/api/v1/articles");
    let r = request();
    let mut response = None;
    for _ in 0..100 {
        match client.post(&url).bearer_auth(DEV).json(&r).send().await {
            Ok(r) => {
                response = Some(r);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    let response = response.expect("companion starts");
    assert_eq!(response.status(), 202);
    let accepted: serde_json::Value = response.json().await.unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let db = Database::open(&path, 4 * 1024 * 1024).unwrap();
    assert_eq!(db.list(None, 100, None).unwrap().0.len(), 1);
    assert!(db.job(accepted["job"].as_str().unwrap()).is_ok());
    drop(db);
    child = Child(start());
    let mut repeated = None;
    for _ in 0..100 {
        match client.post(&url).bearer_auth(DEV).json(&r).send().await {
            Ok(r) => {
                repeated = Some(r);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    let response = repeated.expect("companion restarts");
    assert_eq!(response.status(), 200);
    let repeated: serde_json::Value = response.json().await.unwrap();
    assert_eq!(repeated["job"], accepted["job"]);
    drop(child);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simulated_receive_lost_ack_then_replay_and_token_display() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let article = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    let lose = Arc::new(AtomicBool::new(true));
    let inject = lose.clone();
    let router = server::device(a.clone()).layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let inject = inject.clone();
            async move {
                if req.uri().path().ends_with("/ack") && inject.swap(false, Ordering::SeqCst) {
                    use axum::response::IntoResponse;
                    return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                next.run(req).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let config = format!("url=http://{addr}\ntoken={DEVICE}\n").into_bytes();
    let image = dir.0.join("sd.img");
    let output = dir.0.join("png");
    let run = || {
        let config = config.clone();
        let image = image.clone();
        let output = output.clone();
        tokio::task::spawn_blocking(move || sim::receive::receive(&config, &image, &output))
    };
    assert!(run().await.unwrap().is_err());
    assert_eq!(a.db.job(&article).unwrap().state, "sending");
    a.db.lock()
        .unwrap()
        .execute("UPDATE jobs SET lease_until=0", [])
        .unwrap();
    assert_eq!(run().await.unwrap().unwrap(), 0);
    assert_eq!(a.db.job(&article).unwrap().state, "delivered");
    let disk = sim::disk::Disk::open(&image).unwrap();
    let manager = embedded_sdmmc_for_test(disk);
    let volume = manager
        .open_raw_volume(embedded_sdmmc::VolumeIdx(0))
        .unwrap();
    let directory = manager.open_root_dir(volume).unwrap();
    let mut store = x3_core::fat::FatStore {
        manager: &manager,
        directory,
    };
    let records: Vec<_> = x3_core::receive::Index::load(&mut store)
        .unwrap()
        .iter()
        .cloned()
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].generation, 1);
    assert!(output.join(format!("{}.png", records[0].item())).exists());
    drop(manager);
    let token = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"scryfall_id":"00000000-0000-4000-8000-000000000001","face":0,"fit":"contain","orientation":"portrait","show":true});
    assert_eq!(
        call(
            server::public(a.clone()),
            "POST",
            "/api/v1/tokens",
            DEV,
            token.to_string()
        )
        .await
        .0,
        202
    );
    server::prepare_one(&a).await.unwrap();
    let token = a.db.list(None, 10, None).unwrap().0.remove(0);
    assert_eq!(run().await.unwrap().unwrap(), 1);
    assert_eq!(a.db.job(&token.job).unwrap().state, "delivered");
    assert_eq!(run().await.unwrap().unwrap(), 0);
    assert_eq!(a.db.job(&token.job).unwrap().state, "displayed");
    let preview = image::open(output.join(format!("{}.png", token.item))).unwrap();
    assert_eq!((preview.width(), preview.height()), (528, 792));
    let delete = a.db.queue_delete(records[0].item()).unwrap();
    assert_eq!(run().await.unwrap().unwrap(), 0);
    assert_eq!(a.db.job(&delete.job).unwrap().state, "delivered");
    task.abort();
}
fn embedded_sdmmc_for_test(
    disk: sim::disk::Disk,
) -> embedded_sdmmc::VolumeManager<sim::disk::Disk, sim::disk::Clock> {
    embedded_sdmmc::VolumeManager::new(disk, sim::disk::Clock)
}
/// Fails the first `.0` image fetches with status `.1`, then behaves like Fixture.
struct Flaky(std::sync::atomic::AtomicU32, u16);
impl Source for Flaky {
    fn image<'a>(
        &'a self,
        spec: &'a TokenSpec,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        use std::sync::atomic::Ordering::SeqCst;
        Box::pin(async move {
            if self
                .0
                .try_update(SeqCst, SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                return Err(x3_companion::Error(self.1, "upstream_failed"));
            }
            Fixture.image(spec).await
        })
    }
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>>
    {
        Fixture.search(q)
    }
}
async fn submit_token(app: &App) -> String {
    let r = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"scryfall_id":"00000000-0000-4000-8000-000000000001","face":0,"fit":"contain","orientation":"auto","show":false});
    let (s, _, b) = call(
        server::public(app.clone()),
        "POST",
        "/api/v1/tokens",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
    serde_json::from_slice::<serde_json::Value>(&b).unwrap()["job"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn retry_now(app: &App) {
    app.db
        .lock()
        .unwrap()
        .execute("UPDATE jobs SET retry_at=0", [])
        .unwrap();
}
#[tokio::test]
async fn transient_preparation_failures_retry_then_fail() {
    let dir = Temp::new();
    let mut a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    // One upstream failure: the job waits, then succeeds without blocking others.
    a.source = Arc::new(Flaky(1.into(), 502));
    let token = submit_token(&a).await;
    assert!(server::prepare_one(&a).await.unwrap());
    assert_eq!(a.db.job(&token).unwrap().state, "accepted");
    let article = submit(&a, &request()).await;
    assert!(server::prepare_one(&a).await.unwrap());
    assert_eq!(a.db.job(&article).unwrap().state, "ready");
    assert!(!server::prepare_one(&a).await.unwrap());
    retry_now(&a);
    assert!(server::prepare_one(&a).await.unwrap());
    assert_eq!(a.db.job(&token).unwrap().state, "ready");
    // Busy conversions are retried too, but only a bounded number of times.
    a.source = Arc::new(Flaky(u32::MAX.into(), 503));
    let token = submit_token(&a).await;
    for _ in 0..x3_companion::db::PREPARE_ATTEMPTS {
        retry_now(&a);
        assert!(server::prepare_one(&a).await.unwrap());
    }
    assert_eq!(a.db.job(&token).unwrap().state, "failed");
    let tries = a.db.history(&token).unwrap();
    let tries = tries.iter().filter(|h| h["state"] == "preparing").count();
    assert_eq!(tries, x3_companion::db::PREPARE_ATTEMPTS as usize);
    // A permanent failure is not retried.
    a.source = Arc::new(Flaky(1.into(), 422));
    let token = submit_token(&a).await;
    assert!(server::prepare_one(&a).await.unwrap());
    assert_eq!(a.db.job(&token).unwrap().state, "failed");
}
fn deliver(app: &App, id: &str) {
    let hash = x3_companion::hash(&app.db.payload(id, true).unwrap());
    app.db
        .ack(id, x3_proto::lines::Ack::Committed(Some(&hash)))
        .unwrap();
}
#[tokio::test]
async fn retention_keeps_item_records_and_releases_quota() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let first = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    deliver(&a, &first);
    let item = a.db.job(&first).unwrap().item;
    let mut r = request();
    r["item"] = item.clone().into();
    let replaced = submit(&a, &r).await;
    server::prepare_one(&a).await.unwrap();
    deliver(&a, &replaced);
    let cancelled = submit(&a, &request()).await;
    a.db.cancel(&cancelled).unwrap();
    let gone = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    deliver(&a, &gone);
    let gone_item = a.db.job(&gone).unwrap().item;
    let delete = a.db.queue_delete(&gone_item).unwrap();
    a.db.device_jobs().unwrap();
    a.db.ack(&delete.job, x3_proto::lines::Ack::Committed(None))
        .unwrap();
    let pending = submit(&a, &request()).await;
    a.db.lock()
        .unwrap()
        .execute("UPDATE jobs SET updated=0", [])
        .unwrap();
    assert_eq!(a.db.prune(30).unwrap(), 4);
    let left: Vec<_> = a.db.list(None, 100, None).unwrap().0;
    let left: Vec<_> = left.iter().map(|j| j.job.as_str()).collect();
    assert_eq!(left, [pending.as_str(), replaced.as_str()]);
    assert_eq!(a.db.payload(&replaced, false).unwrap_err().0, 404);
    assert_eq!(a.db.prune(30).unwrap(), 0);
    // The item still on the device can be replaced or removed; a deleted one cannot.
    a.db.queue_delete(&item).unwrap();
    let mut r = request();
    r["item"] = item.into();
    submit(&a, &r).await;
    assert_eq!(a.db.queue_delete(&gone_item).unwrap_err().0, 404);
    let backup = dir.0.join("backup.sqlite");
    a.db.backup(&backup).unwrap();
    Database::restore_check(&backup).unwrap();
}
#[tokio::test]
async fn finished_jobs_do_not_reserve_quota() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 1024 * 1024);
    let ids = [
        submit(&a, &request()).await,
        submit(&a, &request()).await,
        submit(&a, &request()).await,
    ];
    let full = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/articles",
        DEV,
        request().to_string(),
    )
    .await;
    assert_eq!(full.0, 507);
    for id in &ids {
        a.db.cancel(id).unwrap();
    }
    submit(&a, &request()).await;
}
#[test]
fn version_one_database_migrates_and_newer_is_refused() {
    let dir = Temp::new();
    let path = dir.0.join("db");
    drop(Database::open(&path, 4 * 1024 * 1024).unwrap());
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch(
        "ALTER TABLE jobs DROP COLUMN attempts; ALTER TABLE jobs DROP COLUMN retry_at;
         ALTER TABLE jobs DROP COLUMN title; DROP TABLE saved; PRAGMA user_version=1;",
    )
    .unwrap();
    drop(raw);
    let db = Database::open(&path, 4 * 1024 * 1024).unwrap();
    let version: i64 = db
        .lock()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 5);
    assert!(
        db.saved().unwrap().is_empty(),
        "version 4 adds the saved shelf"
    );
    db.lock()
        .unwrap()
        .execute_batch("PRAGMA user_version=6")
        .unwrap();
    drop(db);
    assert_eq!(
        Database::open(&path, 4 * 1024 * 1024).err().unwrap().1,
        "unsupported_database"
    );
}
/// Serves fixed pages: `https://ok.example/…` succeeds, `…/down` is an
/// upstream failure, anything else cannot be read.
struct Pages;
impl x3_companion::link::Links for Pages {
    fn page<'a>(
        &'a self,
        url: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<x3_companion::link::Page>> + Send + 'a>,
    > {
        Box::pin(async move {
            if url.ends_with("/down") {
                Err(x3_companion::Error(502, "upstream_failed"))
            } else if url.starts_with("https://ok.example/") {
                Ok(x3_companion::link::Page {
                    title: "Zażółć gęślą jaźń".into(),
                    text: "# Part one\n\nPchnąć w tę łódź jeża.".into(),
                })
            } else {
                Err(x3_companion::Error(422, "conversion_failed"))
            }
        })
    }
}
async fn submit_link(app: &App, url: &str) -> (u16, String) {
    let r = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"kind":"article","url":url});
    let (s, _, b) = call(
        server::public(app.clone()),
        "POST",
        "/api/v1/articles/link",
        DEV,
        r.to_string(),
    )
    .await;
    let job = serde_json::from_slice::<serde_json::Value>(&b)
        .ok()
        .and_then(|v| v["job"].as_str().map(str::to_owned))
        .unwrap_or_default();
    (s, job)
}
#[tokio::test]
async fn article_from_link_prepares_retries_and_titles() {
    let dir = Temp::new();
    let mut a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    a.links = Arc::new(Pages);
    let (s, ok) = submit_link(&a, "https://ok.example/story").await;
    assert_eq!(s, 202);
    assert_eq!(
        a.db.job(&ok).unwrap().title.as_deref(),
        Some("https://ok.example/story")
    );
    let (_, down) = submit_link(&a, "https://ok.example/down").await;
    let (_, bad) = submit_link(&a, "https://paywall.example/x").await;
    for _ in 0..3 {
        server::prepare_one(&a).await.unwrap();
    }
    let job = a.db.job(&ok).unwrap();
    assert_eq!(job.state, "ready");
    assert_eq!(job.title.as_deref(), Some("Zażółć gęślą jaźń"));
    let article = x3_proto::payload::Article::decode(&a.db.payload(&ok, false).unwrap())
        .unwrap()
        .text
        .to_owned();
    assert!(article.starts_with("# Part one"));
    assert_eq!(a.db.job(&down).unwrap().state, "accepted");
    assert_eq!(a.db.job(&bad).unwrap().state, "failed");
    // Links the companion must never fetch are refused before anything is stored.
    for url in [
        "http://192.168.1.20:8123/",
        "http://localhost/",
        "http://n8n:5678/",
        "https://ok.example:8443/",
        "file:///etc/passwd",
        "not a link",
    ] {
        assert_eq!(submit_link(&a, url).await.0, 400, "{url}");
    }
    assert_eq!(a.db.list(None, 100, None).unwrap().0.len(), 3);
    // Delete jobs carry the item's title so the inbox stays readable.
    let item = a.db.job(&ok).unwrap().item;
    let delete = a.db.queue_delete(&item).unwrap();
    assert_eq!(delete.title.as_deref(), Some("Zażółć gęślą jaźń"));
}
#[tokio::test]
async fn token_name_is_the_title_and_validated() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let mut r = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"scryfall_id":"00000000-0000-4000-8000-000000000001","face":0,"fit":"contain","orientation":"auto","show":false,"name":"Soldier"});
    let (s, _, b) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/tokens",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 202);
    let job = serde_json::from_slice::<serde_json::Value>(&b).unwrap()["job"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(a.db.job(&job).unwrap().title.as_deref(), Some("Soldier"));
    r["submission_id"] = uuid::Uuid::new_v4().to_string().into();
    r["name"] = "bad\nname".into();
    let (s, _, _) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/tokens",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 400);
}
#[tokio::test]
async fn fonts_and_page_headers() {
    let dir = Temp::new();
    let mut a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let (s, h, b) = call(
        server::public(a.clone()),
        "GET",
        "/fonts/literata-latin-wght-normal.woff2",
        DEV,
        "",
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(h["content-type"], "font/woff2");
    assert!(h["cache-control"].to_str().unwrap().contains("immutable"));
    assert!(b.starts_with(b"wOF2"));
    // General Sans may not be redistributed: only from a private folder.
    let path = "/fonts/GeneralSans-Variable.woff2";
    assert_eq!(
        call(server::public(a.clone()), "GET", path, DEV, "")
            .await
            .0,
        404
    );
    std::fs::write(dir.0.join("GeneralSans-Variable.woff2"), b"wOF2test").unwrap();
    a.fonts = Some(dir.0.clone());
    let (s, _, b) = call(server::public(a.clone()), "GET", path, DEV, "").await;
    assert_eq!((s, b.as_slice()), (200, b"wOF2test".as_slice()));
    for bad in ["/fonts/..%2Fdb", "/fonts/inbox.sqlite", "/fonts/"] {
        assert_eq!(
            call(server::public(a.clone()), "GET", bad, DEV, "").await.0,
            404,
            "{bad}"
        );
    }
    let (s, h, _) = call(server::public(a.clone()), "GET", "/", DEV, "").await;
    assert_eq!(s, 200);
    let csp = h["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("style-src 'self'") && !csp.contains("unsafe-inline"));
    assert_eq!(h["cache-control"], "no-store");
    for css in ["/app.css", "/tokens.css"] {
        let (s, h, _) = call(server::public(a.clone()), "GET", css, DEV, "").await;
        assert_eq!(
            (s, h["content-type"].to_str().unwrap()),
            (200, "text/css; charset=utf-8")
        );
    }
    // Static files still need the login.
    assert_eq!(
        call(server::public(a.clone()), "GET", "/tokens.css", "wrong", "")
            .await
            .0,
        401
    );
}
#[tokio::test]
async fn job_preview_pages_through_a_long_article() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let mut r = request();
    r["text"] = (1..=60)
        .map(|n| {
            format!("Paragraph {n}. Pchnąć w tę łódź jeża lub ośm skrzyń fig, a potem jeszcze raz.")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        .into();
    let id = submit(&a, &r).await;
    server::prepare_one(&a).await.unwrap();
    let path = |p: &str| format!("/api/v1/jobs/{id}/preview.png{p}");
    let (s, h, b) = call(server::public(a.clone()), "GET", &path(""), DEV, "").await;
    assert_eq!(s, 200);
    assert!(b.starts_with(b"\x89PNG"));
    let pages: usize = h["x3-page-count"].to_str().unwrap().parse().unwrap();
    assert!(pages >= 5, "{pages}");
    let (s, _, last) = call(
        server::public(a.clone()),
        "GET",
        &path(&format!("?page={}", pages - 1)),
        DEV,
        "",
    )
    .await;
    assert_eq!(s, 200);
    assert_ne!(last, b);
    let beyond = call(
        server::public(a.clone()),
        "GET",
        &path(&format!("?page={pages}")),
        DEV,
        "",
    )
    .await
    .0;
    assert_eq!(beyond, 404);
    for bad in ["?page=-1", "?page=x", "?page=1&page=2", "?other=1"] {
        assert_eq!(
            call(server::public(a.clone()), "GET", &path(bad), DEV, "")
                .await
                .0,
            400,
            "{bad}"
        );
    }
}

/// A signed request as the device sends it (ADR 026). Returns status, headers and body.
type Tweak = Box<dyn FnOnce(&mut axum::http::request::Builder, &mut String)>;

async fn signed(
    app: &App,
    method: &str,
    path: &str,
    nonce: &str,
    body: &str,
    edit: impl FnOnce(&mut axum::http::request::Builder, &mut String),
) -> (u16, axum::http::HeaderMap, Vec<u8>) {
    let key = x3_proto::sign::key(DEVICE);
    let mac = x3_proto::sign::request_mac(&key, method, path, nonce, body.as_bytes());
    let mut mac_hex = [0u8; 64];
    x3_proto::sign::hex(&mac, &mut mac_hex);
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x3-nonce", nonce)
        .header(
            "authorization",
            format!("X3-HMAC {}", std::str::from_utf8(&mac_hex).unwrap()),
        );
    let mut body = body.to_string();
    edit(&mut builder, &mut body);
    let response = server::device_signed(app.clone())
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}

fn reply_mac_ok(nonce: &str, status: u16, headers: &axum::http::HeaderMap, body: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    let Some(m) = headers.get("x3-mac").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let mut got = [0u8; 32];
    if x3_proto::sign::decode_hex(m, &mut got).is_none() {
        return false;
    }
    let digest: [u8; 32] = Sha256::digest(body).into();
    let want = x3_proto::sign::response_mac(&x3_proto::sign::key(DEVICE), nonce, status, &digest);
    x3_proto::sign::same(&got, &want)
}

const N1: &str = "00112233445566778899aabbccddeeff";
const HELLO_LINE: &str = "hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1 free_kib=1 battery=na\n";

#[tokio::test]
async fn the_signed_listener_answers_a_good_request_with_a_signed_reply() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let (s, h, b) = signed(&a, "POST", "/device/v1/hello", N1, HELLO_LINE, |_, _| {}).await;
    assert_eq!(s, 200);
    assert!(String::from_utf8_lossy(&b).starts_with("ok 1 time="));
    assert!(
        reply_mac_ok(N1, s, &h, &b),
        "the reply must be signed for this nonce"
    );
    // A route that does not exist is signed too, so the device can trust the 404.
    let (s, h, b) = signed(
        &a,
        "GET",
        "/device/v1/nothing",
        "11112233445566778899aabbccddeeff",
        "",
        |_, _| {},
    )
    .await;
    assert_eq!(s, 404);
    assert!(reply_mac_ok("11112233445566778899aabbccddeeff", s, &h, &b));
}

#[tokio::test]
async fn the_signed_listener_refuses_replays_forgeries_and_bearer_tokens() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let (s, _, _) = signed(&a, "POST", "/device/v1/hello", N1, HELLO_LINE, |_, _| {}).await;
    assert_eq!(s, 200);
    // The same request again: refused, and the refusal carries no MAC.
    let (s, h, _) = signed(&a, "POST", "/device/v1/hello", N1, HELLO_LINE, |_, _| {}).await;
    assert_eq!(s, 401);
    assert!(h.get("x3-mac").is_none());
    // Each tweak makes the MAC wrong.
    let n2 = "22112233445566778899aabbccddeeff";
    let cases: Vec<(&str, Tweak)> = vec![
        ("body changed", Box::new(|_, b: &mut String| b.push('x'))),
        (
            "bearer instead",
            Box::new(|b: &mut axum::http::request::Builder, _| {
                b.headers_mut()
                    .unwrap()
                    .insert("authorization", format!("Bearer {DEVICE}").parse().unwrap());
            }),
        ),
        (
            "no nonce",
            Box::new(|b: &mut axum::http::request::Builder, _| {
                b.headers_mut().unwrap().remove("x3-nonce");
            }),
        ),
        (
            "upper-case nonce",
            Box::new(|b: &mut axum::http::request::Builder, _| {
                b.headers_mut()
                    .unwrap()
                    .insert("x3-nonce", n2.to_uppercase().parse().unwrap());
            }),
        ),
        (
            "short mac",
            Box::new(|b: &mut axum::http::request::Builder, _| {
                b.headers_mut()
                    .unwrap()
                    .insert("authorization", "X3-HMAC abcd".parse().unwrap());
            }),
        ),
    ];
    for (name, edit) in cases {
        let (s, h, _) = signed(&a, "POST", "/device/v1/hello", n2, HELLO_LINE, edit).await;
        assert_eq!(s, 401, "{name}");
        assert!(h.get("x3-mac").is_none(), "{name}");
    }
    // A query string is not covered by the MAC, so it is refused outright.
    let (s, _, _) = signed(
        &a,
        "GET",
        "/device/v1/jobs?x=1",
        "33112233445566778899aabbccddeeff",
        "",
        |_, _| {},
    )
    .await;
    assert_eq!(s, 401);
    // The old Bearer listener does not accept a signed request.
    let key = x3_proto::sign::key(DEVICE);
    let mac = x3_proto::sign::request_mac(
        &key,
        "GET",
        "/device/v1/jobs",
        "44112233445566778899aabbccddeeff",
        b"",
    );
    let mut hexed = [0u8; 64];
    x3_proto::sign::hex(&mac, &mut hexed);
    let response = server::device(a.clone())
        .oneshot(
            Request::builder()
                .uri("/device/v1/jobs")
                .header("x3-nonce", "44112233445566778899aabbccddeeff")
                .header(
                    "authorization",
                    format!("X3-HMAC {}", std::str::from_utf8(&hexed).unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
}

#[tokio::test]
async fn a_signed_session_delivers_a_job_and_the_payload_hash_matches() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let id = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    let (s, h, b) = signed(
        &a,
        "GET",
        "/device/v1/jobs",
        "55112233445566778899aabbccddeeff",
        "",
        |_, _| {},
    )
    .await;
    assert_eq!(s, 200);
    assert!(reply_mac_ok("55112233445566778899aabbccddeeff", s, &h, &b));
    assert!(String::from_utf8_lossy(&b).contains(&id));
    let path = format!("/device/v1/jobs/{id}/payload");
    let (s, h, payload) = signed(
        &a,
        "GET",
        &path,
        "66112233445566778899aabbccddeeff",
        "",
        |_, _| {},
    )
    .await;
    assert_eq!(s, 200);
    assert!(reply_mac_ok(
        "66112233445566778899aabbccddeeff",
        s,
        &h,
        &payload
    ));
    assert_eq!(
        x3_companion::hash(&payload),
        a.db.job(&id).unwrap().sha256.unwrap()
    );
}

#[tokio::test]
async fn a_session_over_the_signed_listener_delivers_with_one_kept_connection() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let article = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    // A real server behind a proxy that counts connections: the whole session
    // must use one.
    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    let served = tokio::spawn(axum::serve(server, server::device_signed(a.clone())).into_future());
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counted = connections.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut client, _) = listener.accept().await.unwrap();
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                if let Ok(mut upstream) = tokio::net::TcpStream::connect(server_addr).await {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                }
            });
        }
    });
    let config = format!("url=https://device.example.org\ntoken={DEVICE}\n").into_bytes();
    let image = dir.0.join("sd.img");
    let output = dir.0.join("png");
    let (c, i, o) = (config.clone(), image.clone(), output.clone());
    let received =
        tokio::task::spawn_blocking(move || sim::receive::receive_via(&c, &i, &o, Some(addr)))
            .await
            .unwrap()
            .unwrap();
    task.abort();
    served.abort();
    assert_eq!(received, 1);
    assert_eq!(a.db.job(&article).unwrap().state, "delivered");
    assert_eq!(
        connections.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one kept-alive connection"
    );
}

async fn upload(
    app: &App,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (u16, axum::http::HeaderMap, Vec<u8>) {
    let response = server::public(app.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("authorization", format!("Bearer {DEV}"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}

fn png_of(w: u32, h: u32) -> Vec<u8> {
    let image = image::GrayImage::from_fn(w, h, |x, y| image::Luma([((x + y) % 256) as u8]));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
async fn images_upload_preview_queue_and_reach_only_im1_devices() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let picture = png_of(300, 200);
    // Preview: the exact pixels and their hash.
    let (s, h, b) = upload(
        &a,
        "/api/v1/images/preview.png?fit=contain&orientation=auto",
        "image/png",
        picture.clone(),
    )
    .await;
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(&b[1..4], b"PNG");
    let hash = h["x3-payload-sha256"].to_str().unwrap().to_owned();
    // Queue with the previewed hash, a name and show.
    let id = uuid::Uuid::new_v4();
    let path = format!(
        "/api/v1/images?submission_id={id}&fit=contain&orientation=auto&show=1&name=Harbour%20at%20dusk&expected_sha256={hash}"
    );
    let (s, _, b) = upload(&a, &path, "image/png", picture.clone()).await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let item = v["item"].as_str().unwrap();
    assert!(item.starts_with("i-harbour-at-dusk-"), "{item}");
    let job = v["job"].as_str().unwrap().to_owned();
    // Converted on upload; the worker only moves the finished payload on.
    server::prepare_one(&a).await.unwrap();
    assert_eq!(a.db.job(&job).unwrap().state, "ready");
    // The same submission again is a repeat; different bytes a conflict.
    let (s, _, _) = upload(&a, &path, "image/png", picture.clone()).await;
    assert_eq!(s, 200);
    let (s, _, _) = upload(&a, &path, "image/png", png_of(301, 200)).await;
    assert_eq!(s, 409);
    // A device that has not said `im1` is not offered the image.
    let (jobs, _) = a.db.device_jobs().unwrap();
    assert!(jobs.iter().all(|j| j.kind != "image"));
    a.db.hello("hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1 free_kib=1 battery=na\n")
        .unwrap();
    let (jobs, _) = a.db.device_jobs().unwrap();
    assert!(jobs.iter().all(|j| j.kind != "image"));
    a.db.hello("hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1,im1 free_kib=1 battery=na\n")
        .unwrap();
    let (jobs, _) = a.db.device_jobs().unwrap();
    let j = jobs
        .iter()
        .find(|j| j.kind == "image")
        .expect("offered to im1");
    assert!(j.show);
    // The payload is a valid picture.
    let bytes = a.db.payload(&j.job, false).unwrap();
    x3_proto::payload::TokenHeader::payload(&bytes).unwrap();
}

#[tokio::test]
async fn image_uploads_refuse_bad_types_sizes_and_hostile_files() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 4 * 1024 * 1024);
    let q = |extra: &str| {
        format!(
            "/api/v1/images?submission_id={}&fit=cover&orientation=portrait{extra}",
            uuid::Uuid::new_v4()
        )
    };
    let picture = png_of(64, 64);
    // Not a picture type (a cross-site form could send these).
    for kind in [
        "application/json",
        "text/plain",
        "multipart/form-data",
        "application/x-www-form-urlencoded",
        "",
    ] {
        let (s, _, _) = upload(&a, &q(""), kind, picture.clone()).await;
        assert_eq!(s, 415, "{kind:?}");
    }
    // Empty, garbage, truncated.
    assert_eq!(upload(&a, &q(""), "image/png", vec![]).await.0, 400);
    assert_eq!(
        upload(&a, &q(""), "image/png", b"not a picture".to_vec())
            .await
            .0,
        422
    );
    assert_eq!(
        upload(
            &a,
            &q(""),
            "image/png",
            picture[..picture.len() / 2].to_vec()
        )
        .await
        .0,
        422
    );
    // Too many pixels for the decoder (a decompression bomb is the same case).
    assert_eq!(
        upload(&a, &q(""), "image/png", png_of(5000, 8)).await.0,
        422
    );
    // Over the upload cap.
    let (s, _, _) = upload(&a, &q(""), "image/png", vec![0u8; 8 * 1024 * 1024 + 1]).await;
    assert_eq!(s, 413);
    // Bad settings and names.
    for extra in [
        "&show=yes",
        "&name=",
        "&expected_sha256=nothex",
        "&unknown=1",
    ] {
        assert_eq!(
            upload(&a, &q(extra), "image/png", picture.clone()).await.0,
            400,
            "{extra}"
        );
    }
    let (s, _, _) = upload(
        &a,
        &format!(
            "/api/v1/images?submission_id={}&fit=stretch&orientation=auto",
            uuid::Uuid::new_v4()
        ),
        "image/png",
        picture.clone(),
    )
    .await;
    assert_eq!(s, 400);
    assert_eq!(
        upload(
            &a,
            "/api/v1/images?fit=cover&orientation=auto",
            "image/png",
            picture.clone()
        )
        .await
        .0,
        400
    );
    // A wrong expected hash is a conflict and queues nothing.
    let (s, _, _) = upload(
        &a,
        &q(&format!("&expected_sha256={}", "0".repeat(64))),
        "image/png",
        picture.clone(),
    )
    .await;
    assert_eq!(s, 409);
    assert!(a.db.list(None, 50, None).unwrap().0.is_empty());
}

fn json_of(b: &[u8]) -> serde_json::Value {
    serde_json::from_slice(b).unwrap()
}

/// Delivers a prepared job the way the device's ack does.
fn ack_delivered(app: &App, job: &str) -> String {
    let sha = app.db.job(job).unwrap().sha256.unwrap();
    app.db
        .ack(job, x3_proto::lines::Ack::Committed(Some(&sha)))
        .unwrap();
    sha
}

#[tokio::test]
async fn saved_shelf_keeps_delivered_tokens_and_sends_them_again() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 16 * 1024 * 1024);
    let get = |path: &'static str| call(server::public(a.clone()), "GET", path, DEV, "");
    // Articles are never kept; a token is kept only once delivered.
    let article = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    ack_delivered(&a, &article);
    let first = submit_token(&a).await;
    server::prepare_one(&a).await.unwrap();
    assert!(a.db.saved().unwrap().is_empty());
    let sha = ack_delivered(&a, &first);
    let saved = a.db.saved().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].id.as_str(), saved[0].kind.as_str()),
        (sha.as_str(), "token")
    );
    // The same picture again is one entry.
    let second = submit_token(&a).await;
    server::prepare_one(&a).await.unwrap();
    ack_delivered(&a, &second);
    let (s, _, b) = get("/api/v1/saved").await;
    assert_eq!(s, 200);
    assert_eq!(json_of(&b)["saved"].as_array().unwrap().len(), 1);
    // Preview and send again: same payload, no lookup, ready at once.
    let path = format!("/api/v1/saved/{sha}/preview.png");
    let (s, h, _) = call(server::public(a.clone()), "GET", &path, DEV, "").await;
    assert_eq!((s, h["content-type"].to_str().unwrap()), (200, "image/png"));
    let send = format!("/api/v1/saved/{sha}/send");
    let body = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"show":true})
        .to_string();
    let (s, _, b) = call(server::public(a.clone()), "POST", &send, DEV, body.clone()).await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
    let job = json_of(&b)["job"].as_str().unwrap().to_owned();
    server::prepare_one(&a).await.unwrap();
    let j = a.db.job(&job).unwrap();
    assert_eq!(
        (j.state.as_str(), j.sha256.as_deref(), j.show),
        ("ready", Some(sha.as_str()), true)
    );
    assert_ne!(
        j.item,
        a.db.job(&first).unwrap().item,
        "a new item on the device"
    );
    // A retry of the same submission is recognised; a changed body conflicts.
    let (s, _, b) = call(server::public(a.clone()), "POST", &send, DEV, body.clone()).await;
    assert_eq!((s, json_of(&b)["job"].as_str()), (200, Some(job.as_str())));
    let changed = body.replace("true", "false");
    assert_eq!(
        call(server::public(a.clone()), "POST", &send, DEV, changed)
            .await
            .0,
        409
    );
    // Hostile and unknown IDs.
    for bad in ["../x", "zz", &"g".repeat(64)] {
        let p = format!("/api/v1/saved/{bad}/send");
        assert!(matches!(
            call(server::public(a.clone()), "POST", &p, DEV, body.clone())
                .await
                .0,
            400 | 404
        ));
    }
    let unknown = format!("/api/v1/saved/{}", "0".repeat(64));
    assert_eq!(
        call(server::public(a.clone()), "DELETE", &unknown, DEV, "")
            .await
            .0,
        404
    );
    // Forget.
    let path = format!("/api/v1/saved/{sha}");
    assert_eq!(
        call(server::public(a.clone()), "DELETE", &path, DEV, "")
            .await
            .0,
        204
    );
    assert!(a.db.saved().unwrap().is_empty());
    assert_eq!(
        call(server::public(a.clone()), "DELETE", &path, DEV, "")
            .await
            .0,
        404
    );
    // The shelf holds the newest SAVED_MAX: fill it, then deliver one more.
    {
        let c = a.db.lock().unwrap();
        for i in 0..x3_companion::db::SAVED_MAX + 5 {
            c.execute(
                "INSERT INTO saved VALUES(?1,'image','old',x'00',1,?2)",
                rusqlite::params![format!("{i:064x}"), i],
            )
            .unwrap();
        }
    }
    ack_delivered(&a, &job);
    let saved = a.db.saved().unwrap();
    assert_eq!(saved.len() as i64, x3_companion::db::SAVED_MAX);
    assert_eq!(saved[0].id, sha, "the newest stays");
}

#[tokio::test]
async fn inbox_lists_ten_then_pages_back() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 16 * 1024 * 1024);
    let mut jobs = Vec::new();
    for _ in 0..12 {
        jobs.push(submit(&a, &request()).await);
    }
    let (s, _, b) = call(
        server::public(a.clone()),
        "GET",
        "/api/v1/jobs?limit=10",
        DEV,
        "",
    )
    .await;
    assert_eq!(s, 200);
    let page = json_of(&b);
    let ids: Vec<&str> = page["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["job"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 10);
    assert_eq!(ids[0], jobs[11], "newest first");
    assert_eq!(page["more"], true);
    let path = format!("/api/v1/jobs?limit=10&before={}", ids[9]);
    let (_, _, b) = call(server::public(a.clone()), "GET", &path, DEV, "").await;
    let older = json_of(&b);
    let ids: Vec<&str> = older["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["job"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [jobs[1].as_str(), jobs[0].as_str()]);
    assert_eq!(older["more"], false);
    for bad in ["x", "../../etc", "01M6EQBVQASTJG9V1SW9GP6GJW0"] {
        let path = format!("/api/v1/jobs?before={bad}");
        assert_eq!(
            call(server::public(a.clone()), "GET", &path, DEV, "")
                .await
                .0,
            400
        );
    }
}

/// Answers a search with one card named after the query it was sent.
struct Echo;
impl Source for Echo {
    fn image<'a>(
        &'a self,
        spec: &'a TokenSpec,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>> + Send + 'a>> {
        Fixture.image(spec)
    }
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value>> + Send + 'a>>
    {
        Box::pin(async move {
            Ok(serde_json::json!({"data":[{"id":"00000000-0000-4000-8000-000000000001","name":q}]}))
        })
    }
}

#[tokio::test]
async fn token_search_asks_for_tokens_only_and_ranks_names() {
    let dir = Temp::new();
    let mut a = app(&dir.0.join("db"), 16 * 1024 * 1024);
    a.source = Arc::new(Echo);
    let (s, _, b) = call(
        server::public(a.clone()),
        "GET",
        "/api/v1/scryfall/search?q=soldier%20or%20goblin",
        DEV,
        "",
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(
        json_of(&b)["data"][0]["name"],
        "t:token (soldier or goblin)"
    );
    for bad in ["", "%20%20", "a%0Ab"] {
        let path = format!("/api/v1/scryfall/search?q={bad}");
        assert_eq!(
            call(server::public(a.clone()), "GET", &path, DEV, "")
                .await
                .0,
            400
        );
    }
    let card = |id: u8, name: &str| {
        serde_json::json!({
            "id": format!("00000000-0000-4000-8000-0000000000{id:02}"), "name": name,
            "set_name": "Set", "released_at": "2024-04-19", "power": "1", "toughness": "1",
            "colors": ["W"], "type_line": "Token Creature — Soldier",
            "image_uris": {"small": "https://cards.scryfall.io/s.jpg", "normal": "https://evil.example/n.jpg"}
        })
    };
    let mut dfc = card(5, "Goblin // Soldier");
    dfc.as_object_mut().unwrap().remove("image_uris");
    dfc["card_faces"] = serde_json::json!([
        {"name":"Goblin","type_line":"Token Creature — Goblin","image_uris":{"small":"https://cards.scryfall.io/g.jpg","normal":"https://cards.scryfall.io/gn.jpg"}},
        {"name":"Soldier","type_line":"Token Creature — Soldier","power":"1","toughness":"1","image_uris":{"small":"https://cards.scryfall.io/b.jpg","normal":"https://cards.scryfall.io/bn.jpg"}}
    ]);
    let found = serde_json::json!({"data":[
        card(1, "Bird Soldier"), card(2, "Kor Soldier"), dfc, card(3, "Soldier"),
        card(4, "Soldierlike"), card(6, "Spirit"), {"id":"not-a-uuid","name":"Soldier"}
    ]});
    let out = server::tokens(&found, "Soldier");
    let names: Vec<&str> = out["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Goblin // Soldier",
            "Soldier",
            "Soldierlike",
            "Bird Soldier",
            "Kor Soldier",
            "Spirit"
        ]
    );
    assert_eq!(out["total"], 6);
    let faces = out["data"][0]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 2, "both sides of a double-faced token");
    assert_eq!(faces[1]["image"], "https://cards.scryfall.io/bn.jpg");
    assert_eq!(faces[1]["pt"], "1/1");
    let plain = &out["data"][1]["faces"][0];
    assert_eq!(
        (plain["pt"].as_str(), plain["thumb"].as_str()),
        (Some("1/1"), Some("https://cards.scryfall.io/s.jpg"))
    );
    assert!(plain["image"].is_null(), "only Scryfall's image host");
    assert_eq!(out["data"][1]["year"], "2024");
}

#[tokio::test]
async fn upgrading_fills_the_shelf_once_with_what_the_reader_has() {
    let dir = Temp::new();
    let path = dir.0.join("db");
    let a = app(&path, 16 * 1024 * 1024);
    let delivered = submit_token(&a).await;
    server::prepare_one(&a).await.unwrap();
    let sha = ack_delivered(&a, &delivered);
    let waiting = submit_token(&a).await;
    let article = submit(&a, &request()).await;
    server::prepare_one(&a).await.unwrap();
    server::prepare_one(&a).await.unwrap();
    ack_delivered(&a, &article);
    // A version 4 database whose shelf was never filled.
    a.db.lock()
        .unwrap()
        .execute_batch("DELETE FROM saved; PRAGMA user_version=4")
        .unwrap();
    drop(a);
    let db = Database::open(&path, 16 * 1024 * 1024).unwrap();
    let saved = db.saved().unwrap();
    assert_eq!(
        saved.len(),
        1,
        "only the delivered token, not the waiting one or the article"
    );
    assert_eq!(saved[0].id, sha);
    assert_ne!(db.job(&waiting).unwrap().state, "delivered");
    // Once only: a forgotten entry does not come back on the next start.
    db.forget(&sha).unwrap();
    drop(db);
    assert!(
        Database::open(&path, 16 * 1024 * 1024)
            .unwrap()
            .saved()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn scryfall_rules_whole_cards_and_rate_gaps() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 16 * 1024 * 1024);
    // Cropping could cut off the artist and copyright line: refused.
    let r = serde_json::json!({"submission_id":uuid::Uuid::new_v4().to_string(),"scryfall_id":"00000000-0000-4000-8000-000000000001","face":0,"fit":"cover","orientation":"auto","show":false});
    let (s, _, _) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/tokens",
        DEV,
        r.to_string(),
    )
    .await;
    assert_eq!(s, 400);
    let p = "/api/v1/tokens/preview.png?scryfall_id=00000000-0000-4000-8000-000000000001&face=0&fit=cover&orientation=auto";
    assert_eq!(
        call(server::public(a.clone()), "GET", p, DEV, "").await.0,
        400
    );
    let gap = |u: &str| x3_companion::upstream::gap(&reqwest::Url::parse(u).unwrap()).as_millis();
    assert_eq!(
        gap("https://api.scryfall.com/cards/search?q=t%3Atoken"),
        500
    );
    assert_eq!(
        gap("https://api.scryfall.com/cards/00000000-0000-4000-8000-000000000001"),
        100
    );
    assert_eq!(gap("https://cards.scryfall.io/png/front/x.png"), 100);
}

/// The bodies docs/SHORTCUTS.md tells the iPhone Shortcuts to send. Shortcuts
/// has no UUID action, so the guide builds one from the date and a random
/// number: digits only, in UUID shape.
#[tokio::test]
async fn shortcut_bodies_from_the_guide_are_accepted() {
    let dir = Temp::new();
    let a = app(&dir.0.join("db"), 16 * 1024 * 1024);
    let link = serde_json::json!({"submission_id":"20261102-1530-4071-8123-482910384756","kind":"article","url":"https://example.org/a-long-read"});
    let (s, _, b) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/articles/link",
        DEV,
        link.to_string(),
    )
    .await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
    assert!(json_of(&b)["job"].is_string());
    // The same submission again (a Shortcut run twice) is recognised.
    let (s, _, _) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/articles/link",
        DEV,
        link.to_string(),
    )
    .await;
    assert_eq!(s, 200);
    let summary = serde_json::json!({"submission_id":"20261102-1531-4072-8456-918273645501","kind":"summary","title":"Inbox digest","source_url":"","text":"First point.\n\nSecond point."});
    let (s, _, b) = call(
        server::public(a.clone()),
        "POST",
        "/api/v1/articles",
        DEV,
        summary.to_string(),
    )
    .await;
    assert_eq!(s, 202, "{}", String::from_utf8_lossy(&b));
}
