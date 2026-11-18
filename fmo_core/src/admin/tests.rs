use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode};
use axum::Router;
use base64::Engine;
use bitcoin::hashes::{sha256, Hash};
use chrono::{DateTime, Utc};
use fmo_api_types::{AdminAuditPage, AdminKeys, AdminRole, AdminSession};
use nostr_sdk::{Event, EventBuilder, Keys, Kind, Tag, Timestamp, ToBech32, Url};
use serde_json::json;
use tower::ServiceExt;

use super::audit::{self, audited, Action, DELEGATE_ADDS_PER_DAY};
use super::auth::{
    credentials, signature_window, verify_signed_request, Admin, AdminAuth, Credentials, Principal,
    MAX_CLOCK_SKEW,
};
use super::keys::{self, valid_label, MAX_DELEGATES};
use crate::error::ClientError;
use crate::federation::observer::FederationObserver;
use crate::source::SourceRegistry;
use crate::AppState;

const TOKEN: &str = "an admin token for tests";
const PUBLIC_URL: &str = "https://observer.example.com/api";

fn url(path: &str) -> Url {
    Url::parse(&format!("{PUBLIC_URL}{path}")).expect("valid")
}

/// A NIP-98 event, as the `Authorization: Nostr` header carries it
struct Signed {
    url: String,
    method: String,
    /// The `payload` tag: the hash of this body
    payload: Option<Vec<u8>>,
    created_at: DateTime<Utc>,
    extra_tags: Vec<Tag>,
    kind: Kind,
}

impl Signed {
    /// Like the frontend, with a nonce: two requests alike in the same
    /// second would otherwise be the same event, and the second a replay
    fn new(method: &Method, path: &str, body: &[u8]) -> Self {
        static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            url: url(path).to_string(),
            method: method.to_string(),
            payload: (!body.is_empty()).then(|| body.to_vec()),
            created_at: Utc::now(),
            extra_tags: vec![Tag::parse(&["nonce", &nonce.to_string()]).expect("valid")],
            kind: Kind::HttpAuth,
        }
    }

    fn event(&self, keys: &Keys) -> Event {
        let mut tags = vec![
            Tag::parse(&["u", &self.url]).expect("valid"),
            Tag::parse(&["method", &self.method]).expect("valid"),
        ];
        if let Some(body) = &self.payload {
            let hash = sha256::Hash::hash(body).to_string();
            tags.push(Tag::parse(&["payload", &hash]).expect("valid"));
        }
        tags.extend(self.extra_tags.iter().cloned());
        EventBuilder::new(self.kind, "", tags)
            .custom_created_at(Timestamp::from(self.created_at.timestamp() as u64))
            .to_event(keys)
            .expect("signs")
    }

    fn header(&self, keys: &Keys) -> String {
        encode(&self.event(keys))
    }
}

fn encode(event: &impl serde::Serialize) -> String {
    base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(event).expect("encodes"))
}

#[test]
fn a_signed_request_authorizes_only_the_request_it_names() {
    let keys = Keys::generate();
    let now = Utc::now();
    let body = br#"{"invite":"fed1"}"#;
    let put = Method::PUT;
    let check = |signed: &Signed, path: &str, method: &Method, body: &[u8]| {
        verify_signed_request(&signed.header(&keys), &url(path), method, body, now)
    };

    let signed = Signed::new(&put, "/federations", body);
    let verified = check(&signed, "/federations", &put, body).expect("valid");
    assert_eq!(verified.pubkey, keys.public_key());

    // Another path, query, method or body
    assert!(check(&signed, "/admin/keys", &put, body).is_err());
    assert!(check(&signed, "/federations?x=1", &put, body).is_err());
    assert!(check(&signed, "/federations", &Method::POST, body).is_err());
    assert!(check(&signed, "/federations", &put, b"{}").is_err());
    assert!(check(&signed, "/federations", &put, b"").is_err());

    // The same path on another site, or over plain HTTP
    for other in [
        "https://evil.example.com/api/federations",
        "http://observer.example.com/api/federations",
        "https://observer.example.com/federations",
        "https://observer.example.com:8443/api/federations",
        "not a url",
    ] {
        let mut elsewhere = Signed::new(&put, "/federations", body);
        elsewhere.url = other.to_owned();
        assert!(
            check(&elsewhere, "/federations", &put, body).is_err(),
            "{other}"
        );
    }

    // A body needs its hash signed; without a body the tag is optional
    let mut unhashed = Signed::new(&put, "/federations", body);
    unhashed.payload = None;
    assert!(check(&unhashed, "/federations", &put, body).is_err());
    let get = Signed::new(&Method::GET, "/admin/session", b"");
    assert!(check(&get, "/admin/session", &Method::GET, b"").is_ok());
    let mut hashed_empty = Signed::new(&Method::GET, "/admin/session", b"");
    hashed_empty.payload = Some(Vec::new());
    assert!(check(&hashed_empty, "/admin/session", &Method::GET, b"").is_ok());

    // Any other kind of event the key ever signed
    let mut note = Signed::new(&put, "/federations", body);
    note.kind = Kind::TextNote;
    assert!(check(&note, "/federations", &put, body).is_err());
}

#[test]
fn a_signed_request_is_accepted_exactly_within_its_window() {
    let keys = Keys::generate();
    // Events carry whole seconds; the server's clock doesn't
    let created_at = DateTime::from_timestamp(1_800_000_000, 0).expect("valid");
    let skew = chrono::Duration::from_std(MAX_CLOCK_SKEW).expect("small");
    let nanosecond = chrono::Duration::nanoseconds(1);
    let get = Method::GET;
    let mut signed = Signed::new(&get, "/admin/session", b"");
    signed.created_at = created_at;
    let header = signed.header(&keys);
    let at =
        |now: DateTime<Utc>| verify_signed_request(&header, &url("/admin/session"), &get, b"", now);

    let (not_before, valid_until) = signature_window(created_at);
    assert_eq!(
        (not_before, valid_until),
        (created_at - skew, created_at + skew)
    );
    assert!(at(not_before).is_ok());
    assert!(at(valid_until).is_ok());
    assert!(at(not_before - nanosecond).is_err());
    assert!(at(valid_until + nanosecond).is_err());
    // The id must be kept until the window's end, whenever it was first used
    assert_eq!(
        at(not_before).map(|signed| signed.valid_until),
        Ok(valid_until)
    );
}

#[test]
fn forged_and_ambiguous_events_are_refused() {
    let keys = Keys::generate();
    let now = Utc::now();
    let get = Method::GET;
    let target = url("/admin/session");
    let verify = |header: &str| verify_signed_request(header, &target, &get, b"", now);

    // Signed for another site, then edited to name this one
    let mut elsewhere = Signed::new(&get, "/admin/session", b"");
    elsewhere.url = "https://evil.example.com/api/admin/session".to_owned();
    let mut forged = serde_json::to_value(elsewhere.event(&keys)).expect("encodes");
    forged["tags"][0][1] = target.to_string().into();
    assert!(verify(&encode(&forged)).is_err());

    // Someone else's signature under the admin's key
    let honest = Signed::new(&get, "/admin/session", b"");
    let mut stolen = serde_json::to_value(honest.event(&Keys::generate())).expect("encodes");
    stolen["pubkey"] = keys.public_key().to_hex().into();
    assert!(verify(&encode(&stolen)).is_err());

    // A second URL or method the signer's user may not have been shown
    for tag in [
        ["u", "https://evil.example.com/"],
        ["u", target.as_str()],
        ["method", "DELETE"],
    ] {
        let mut repeated = Signed::new(&get, "/admin/session", b"");
        repeated.extra_tags.push(Tag::parse(&tag).expect("valid"));
        assert!(verify(&repeated.header(&keys)).is_err(), "{tag:?}");
    }

    // Not an event at all
    for header in [
        "",
        "%%%",
        &encode(&json!({ "kind": 27235 })),
        "bm90IGpzb24=",
    ] {
        assert!(verify(header).is_err(), "{header:?}");
    }
    assert!(verify(&honest.header(&keys)).is_ok());
}

#[test]
fn authorization_headers_are_parsed_strictly() {
    let headers = |values: &[&str]| {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                header::AUTHORIZATION,
                HeaderValue::from_str(value).expect("valid"),
            );
        }
        headers
    };
    let status = |values: &[&str]| {
        credentials(&headers(values))
            .err()
            .map(|error| error.status())
    };

    assert_eq!(
        credentials(&headers(&["Bearer secret"])).ok(),
        Some(Credentials::Bearer("secret"))
    );
    assert_eq!(
        credentials(&headers(&["nostr  abc= "])).ok(),
        Some(Credentials::Nostr("abc="))
    );
    for refused in [
        &[][..],
        &["Bearer a", "Nostr b"],
        &["Basic dXNlcjpwYXNz"],
        &["Bearer"],
        &[&format!("Nostr {}", "a".repeat(5000))],
    ] {
        assert_eq!(
            status(refused),
            Some(StatusCode::UNAUTHORIZED),
            "{refused:?}"
        );
    }
}

#[test]
fn admin_settings_are_validated() {
    let owner = Keys::generate().public_key();
    let npub = owner.to_bech32().expect("encodes");
    let other = Keys::generate().public_key();

    let auth = AdminAuth::new(
        TOKEN,
        Some(&format!("{npub}, {}", other.to_hex())),
        Some("https://observer.example.com/api/"),
    )
    .expect("valid");
    assert_eq!(auth.owners().count(), 2);
    assert!(auth.is_owner(&owner) && auth.is_owner(&other));
    assert!(!auth.is_owner(&Keys::generate().public_key()));

    // Without a public URL only the token works
    let token_only = AdminAuth::new(TOKEN, None, None).expect("valid");
    assert_eq!(token_only.owners().count(), 0);
    // A URL without owners: delegates granted with the token can sign in
    assert!(AdminAuth::new(TOKEN, None, Some(PUBLIC_URL)).is_ok());
    assert!(AdminAuth::new(TOKEN, Some(""), Some("http://127.0.0.1:3000")).is_ok());
    assert!(AdminAuth::new(TOKEN, Some(""), Some("http://localhost:3000")).is_ok());
    assert!(AdminAuth::new(TOKEN, Some(""), Some("http://[::1]:3000")).is_ok());

    assert!(AdminAuth::new("", None, None).is_err());
    assert!(AdminAuth::new(TOKEN, Some(&npub), None).is_err());
    // A bad entry is named by its position, never echoed: it may be a secret
    let secret = Keys::generate()
        .secret_key()
        .expect("generated keys have one")
        .to_bech32()
        .expect("encodes");
    for (list, position) in [
        (format!("{npub}, npub1nonsense"), "entry 2"),
        (secret.clone(), "entry 1"),
    ] {
        let error = AdminAuth::new(TOKEN, Some(&list), Some(PUBLIC_URL))
            .expect_err("invalid")
            .to_string();
        assert!(error.contains(position), "{error}");
        assert!(
            !error.contains("nonsense") && !error.contains(&secret),
            "{error}"
        );
    }
    for bad_url in [
        "http://observer.example.com/api",
        "ftp://observer.example.com",
        "https://observer.example.com/api?x=1",
        "https://observer.example.com/api#x",
        "https://user:pass@observer.example.com/api",
        "observer.example.com",
    ] {
        assert!(
            AdminAuth::new(TOKEN, Some(&npub), Some(bad_url)).is_err(),
            "{bad_url}"
        );
    }
    let many = (0..17)
        .map(|_| Keys::generate().public_key().to_hex())
        .collect::<Vec<_>>()
        .join(",");
    assert!(AdminAuth::new(TOKEN, Some(&many), Some(PUBLIC_URL)).is_err());

    // Nothing of the token is shown
    assert!(!format!("{auth:?}").contains(TOKEN));
}

#[test]
fn labels_are_bounded_text() {
    assert_eq!(valid_label("  Alice  ").ok(), Some("Alice"));
    assert!(valid_label(&"a".repeat(64)).is_ok());
    for bad in ["", "   ", &"a".repeat(65), "line\nbreak", "nul\u{0}"] {
        assert!(valid_label(bad).is_err(), "{bad:?}");
    }
}

/// A schema of a test's own in `FMO_ADMIN_TEST_DATABASE`, holding the admin
/// tables
struct TestSchema {
    database: String,
    name: String,
    client: tokio_postgres::Client,
    driver: tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
}

impl TestSchema {
    /// Tests run in parallel, so each names its own schema
    async fn new(test: &str) -> anyhow::Result<Self> {
        let database = std::env::var("FMO_ADMIN_TEST_DATABASE")?;
        let (client, driver) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
        let driver = tokio::spawn(driver);
        let name = format!("fmo_admin_test_{}_{test}", std::process::id());
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {name}; SET search_path TO {name};
                 CREATE TABLE schema_version(version INTEGER);
                 CREATE TABLE federations(federation_id BYTEA PRIMARY KEY, config BYTEA NOT NULL);
                 {}",
                include_str!("../../schema/v14.sql")
            ))
            .await?;
        Ok(Self {
            database,
            name,
            client,
            driver,
        })
    }

    /// An observer whose connections see only this schema. As a worker, it
    /// takes administrative requests.
    async fn observer(&self, worker: bool) -> anyhow::Result<FederationObserver> {
        let pool = deadpool_postgres::Config {
            url: Some(self.database.clone()),
            options: Some(format!("-c search_path={}", self.name)),
            ..Default::default()
        }
        .create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        )?;
        let observer = FederationObserver::for_tests(pool, SourceRegistry::default()).await?;
        if !worker {
            return Ok(observer);
        }
        let (lease, driver) =
            tokio_postgres::connect(&self.database, tokio_postgres::NoTls).await?;
        tokio::spawn(driver);
        Ok(observer.with_worker_lease(lease))
    }

    async fn finish(self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        self.client
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.name))
            .await?;
        drop(self.client);
        self.driver.await??;
        result
    }
}

fn owner_admin(keys: &Keys) -> Admin {
    Admin {
        principal: Principal::Key(keys.public_key()),
        role: AdminRole::Owner,
        grant: None,
    }
}

fn status_of<T>(result: anyhow::Result<T>) -> Option<StatusCode> {
    result
        .err()
        .and_then(|error| error.downcast_ref::<ClientError>().map(ClientError::status))
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_grants_expire_are_replaced_and_revoked() -> anyhow::Result<()> {
    let db = TestSchema::new("grants").await?;
    let result = async {
        let observer = db.observer(true).await?;
        let mut connection = observer.connection().await?;
        let owner = owner_admin(&Keys::generate());
        let alice = Keys::generate().public_key();
        let now = Utc::now();

        assert!(keys::active_delegate(&connection, &alice, now)
            .await?
            .is_none());
        let granted = keys::grant(&mut connection, &owner, &alice, " Alice ", None).await?;
        assert_eq!(granted.label, "Alice");
        assert_eq!(granted.granted_by, owner.principal.key().copied());
        assert!(keys::active_delegate(&connection, &alice, now)
            .await?
            .is_some());

        // Granting again replaces the label and expiry, in the one row
        let soon = now + chrono::Duration::hours(1);
        keys::grant(&mut connection, &owner, &alice, "Alice B", Some(soon)).await?;
        let delegates = keys::list_delegates(&connection).await?;
        assert_eq!(delegates.len(), 1);
        assert_eq!(delegates[0].label, "Alice B");
        assert!(keys::active_delegate(&connection, &alice, now)
            .await?
            .is_some());
        // Past its expiry the grant stops working, and is still listed
        let later = soon + chrono::Duration::seconds(1);
        assert!(keys::active_delegate(&connection, &alice, later)
            .await?
            .is_none());
        assert!(keys::list_delegates(&connection).await?[0].expired(later));

        assert!(keys::revoke(&mut connection, &owner, &alice).await?);
        assert!(!keys::revoke(&mut connection, &owner, &alice).await?);
        assert!(keys::active_delegate(&connection, &alice, now)
            .await?
            .is_none());

        // Bad labels store nothing
        assert_eq!(
            status_of(keys::grant(&mut connection, &owner, &alice, "", None).await),
            Some(StatusCode::BAD_REQUEST)
        );
        assert!(keys::list_delegates(&connection).await?.is_empty());

        // The cap holds, and a key that already has a grant can still renew
        for index in 0..MAX_DELEGATES {
            let key = Keys::generate().public_key();
            keys::grant(&mut connection, &owner, &key, &format!("k{index}"), None).await?;
        }
        let renewed = keys::list_delegates(&connection).await?[0].pubkey;
        keys::grant(&mut connection, &owner, &renewed, "renewed", None).await?;
        assert_eq!(
            status_of(keys::grant(&mut connection, &owner, &alice, "one more", None).await),
            Some(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            keys::list_delegates(&connection).await?.len() as i64,
            MAX_DELEGATES
        );

        // Every grant and revoke is on record, newest first, in pages
        let (page, next) = audit::list(&connection, None, 10).await.map_err(failed)?;
        assert_eq!(page.len(), 10);
        assert_eq!(page[0].action, "key.grant");
        assert_eq!(page[0].outcome, "succeeded");
        assert_eq!(page[0].detail["label"], "renewed");
        let (older, _) = audit::list(&connection, next, 500).await.map_err(failed)?;
        // 2 grants + 1 revoke of Alice, 64 grants and a renewal
        assert_eq!(page.len() + older.len(), 68);
        assert!(older.iter().all(|entry| entry.id < page[9].id));
        assert!(older.iter().any(|entry| entry.action == "key.revoke"));
        Ok(())
    }
    .await;
    db.finish(result).await
}

fn failed(_: crate::error::AppError) -> anyhow::Error {
    anyhow::anyhow!("The query failed")
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_signed_requests_are_used_once() -> anyhow::Result<()> {
    let db = TestSchema::new("replay").await?;
    let result = async {
        let observer = db.observer(true).await?;
        let connection = observer.connection().await?;
        let created_at = DateTime::from_timestamp(1_800_000_000, 0).expect("valid");
        let (not_before, valid_until) = signature_window(created_at);
        let microsecond = chrono::Duration::microseconds(1);
        let (first, second) = ([1u8; 32], [2u8; 32]);

        // First used as early as its signature is accepted
        assert!(keys::record_signed_request(&connection, &first, valid_until, not_before).await?);
        assert!(!keys::record_signed_request(&connection, &first, valid_until, not_before).await?);
        assert!(keys::record_signed_request(&connection, &second, valid_until, not_before).await?);
        // A replay at the last instant its signature is accepted still finds it
        assert!(!keys::record_signed_request(&connection, &first, valid_until, valid_until).await?);

        // Once the window has ended the ids are forgotten, and the table
        // doesn't grow; the signatures are refused by then anyway
        let later = valid_until + microsecond;
        let (_, third_until) = signature_window(created_at + chrono::Duration::seconds(61));
        assert!(keys::record_signed_request(&connection, &[3u8; 32], third_until, later).await?);
        let kept: Vec<Vec<u8>> = connection
            .query("SELECT event_id FROM admin_auth_events", &[])
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect();
        assert_eq!(kept, vec![vec![3u8; 32]]);
        Ok(())
    }
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_delegates_add_a_limited_number_of_federations() -> anyhow::Result<()> {
    let db = TestSchema::new("limit").await?;
    let result = async {
        let observer = db.observer(true).await?;
        let mut connection = observer.connection().await?;
        let owner = owner_admin(&Keys::generate());
        let delegate_key = Keys::generate().public_key();
        let grant = keys::grant(&mut connection, &owner, &delegate_key, "Alice", None).await?;
        let delegate = Admin {
            principal: Principal::Key(delegate_key),
            role: AdminRole::Delegate,
            grant: Some(grant),
        };
        let add = |admin: &Admin, works: bool| {
            let (observer, admin) = (observer.clone(), admin.clone());
            async move {
                audited(
                    &observer,
                    &admin,
                    Action::FederationAdd,
                    "00",
                    json!({}),
                    async move {
                        anyhow::ensure!(works, "unreachable guardians");
                        Ok(())
                    },
                )
                .await
            }
        };

        // Failed attempts count too: each one connected somewhere
        for attempt in 0..DELEGATE_ADDS_PER_DAY {
            let works = attempt % 2 == 0;
            assert_eq!(add(&delegate, works).await.is_ok(), works);
        }
        assert_eq!(
            status_of(add(&delegate, true).await),
            Some(StatusCode::TOO_MANY_REQUESTS)
        );
        // The refused attempt ran nothing and is not on record
        let recorded = |outcome: &'static str| {
            let connection = &connection;
            async move {
                let count: i64 = connection
                    .query_one(
                        "SELECT COUNT(*) FROM admin_audit_log
                         WHERE action = 'federation.add' AND outcome = $1",
                        &[&outcome],
                    )
                    .await?
                    .get(0);
                anyhow::Ok(count)
            }
        };
        assert_eq!(recorded("succeeded").await?, DELEGATE_ADDS_PER_DAY / 2);
        assert_eq!(recorded("failed").await?, DELEGATE_ADDS_PER_DAY / 2);
        assert_eq!(recorded("started").await?, 0);

        // Attempts older than a day no longer count
        connection
            .execute(
                "UPDATE admin_audit_log SET at = at - interval '25 hours'
                 WHERE id = (SELECT MIN(id) FROM admin_audit_log WHERE action = 'federation.add')",
                &[],
            )
            .await?;
        assert!(add(&delegate, true).await.is_ok());

        // Owners have no limit
        for _ in 0..DELEGATE_ADDS_PER_DAY + 1 {
            assert!(add(&owner, true).await.is_ok());
        }

        // A grant revoked after the request was authenticated stops it here
        assert!(keys::revoke(&mut connection, &owner, &delegate_key).await?);
        assert_eq!(
            status_of(add(&delegate, true).await),
            Some(StatusCode::FORBIDDEN)
        );
        Ok(())
    }
    .await;
    db.finish(result).await
}

/// The admin API and the federation routes as the server mounts them, with
/// the given owner keys
struct Api {
    router: Router,
}

impl Api {
    fn new(observer: FederationObserver, owner: &Keys) -> anyhow::Result<Self> {
        Self::with_owners(observer, &[owner])
    }

    fn with_owners(observer: FederationObserver, owners: &[&Keys]) -> anyhow::Result<Self> {
        let owners = owners
            .iter()
            .map(|keys| keys.public_key().to_hex())
            .collect::<Vec<_>>()
            .join(",");
        let state = AppState {
            federation_config_cache: Default::default(),
            meta_override_cache: Default::default(),
            consensus_meta_cache: Default::default(),
            federation_observer: observer,
            http_metrics: Arc::new(crate::http::HttpMetrics::new(1)),
            admin: Arc::new(AdminAuth::new(TOKEN, Some(&owners), Some(PUBLIC_URL))?),
        };
        Ok(Self {
            router: super::routes()
                .nest("/federations", crate::federation::get_federations_routes())
                .with_state(state),
        })
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        authorization: Option<String>,
        body: &[u8],
    ) -> anyhow::Result<(StatusCode, HeaderMap, serde_json::Value)> {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body.to_vec()))?)
            .await?;
        let (parts, body) = response.into_parts();
        let body = axum::body::to_bytes(body, 1 << 20).await?;
        let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        Ok((parts.status, parts.headers, json))
    }

    /// A request signed by `keys`, as the frontend sends it
    async fn signed(
        &self,
        keys: &Keys,
        method: Method,
        path: &str,
        body: &[u8],
    ) -> anyhow::Result<(StatusCode, serde_json::Value)> {
        let header = Signed::new(&method, path, body).header(keys);
        let (status, _, json) = self
            .send(method, path, Some(format!("Nostr {header}")), body)
            .await?;
        Ok((status, json))
    }
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_admin_api_enforces_roles_and_revocation() -> anyhow::Result<()> {
    let db = TestSchema::new("api").await?;
    let result = async {
        let (owner, alice, stranger) = (Keys::generate(), Keys::generate(), Keys::generate());
        let api = Api::new(db.observer(true).await?, &owner)?;
        let (get, put, delete) = (Method::GET, Method::PUT, Method::DELETE);
        let alice_path = format!("/admin/keys/{}", alice.public_key().to_hex());
        let grant = serde_json::to_vec(&json!({ "label": "Alice" }))?;

        // No credentials, wrong ones, and a valid signature of an unknown key
        let (status, headers, _) = api.send(get.clone(), "/admin/session", None, b"").await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        let wrong = Some("Bearer not the token".to_owned());
        let (status, _, _) = api.send(get.clone(), "/admin/session", wrong, b"").await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = api
            .signed(&stranger, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = api
            .signed(&alice, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The owner, by key and by token
        let (status, session) = api
            .signed(&owner, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::OK);
        let session: AdminSession = serde_json::from_value(session)?;
        assert_eq!(session.role, AdminRole::Owner);
        assert_eq!(
            session.key.map(|key| key.pubkey),
            Some(owner.public_key().to_hex())
        );
        let token = Some(format!("Bearer {TOKEN}"));
        let (status, headers, session) = api
            .send(get.clone(), "/admin/session", token.clone(), b"")
            .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(session["role"], "owner");
        assert_eq!(session["key"], serde_json::Value::Null);

        // A captured header doesn't work twice
        let header = Signed::new(&get, "/admin/session", b"").header(&owner);
        let replayed = Some(format!("Nostr {header}"));
        let (status, _, _) = api
            .send(get.clone(), "/admin/session", replayed.clone(), b"")
            .await?;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = api
            .send(get.clone(), "/admin/session", replayed, b"")
            .await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // The owner grants Alice access; a body other than the signed one,
        // an owner's own key and bad input are refused
        let header = Signed::new(&put, &alice_path, &grant).header(&owner);
        let swapped = serde_json::to_vec(&json!({ "label": "Mallory" }))?;
        let (status, _, _) = api
            .send(
                put.clone(),
                &alice_path,
                Some(format!("Nostr {header}")),
                &swapped,
            )
            .await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, granted) = api.signed(&owner, put.clone(), &alice_path, &grant).await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(granted["label"], "Alice");
        assert_eq!(granted["granted_by"]["pubkey"], owner.public_key().to_hex());
        let own_path = format!("/admin/keys/{}", owner.public_key().to_bech32()?);
        let (status, _) = api.signed(&owner, put.clone(), &own_path, &grant).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = api
            .signed(&owner, put.clone(), "/admin/keys/nonsense", &grant)
            .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let past =
            serde_json::to_vec(&json!({ "label": "x", "expires_at": "2020-01-01T00:00:00Z" }))?;
        let (status, _) = api.signed(&owner, put.clone(), &alice_path, &past).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Alice is a delegate now: she has a session, and none of the owners'
        // endpoints
        let (status, session) = api
            .signed(&alice, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(session["role"], "delegate");
        assert_eq!(session["label"], "Alice");
        let mallory_path = format!("/admin/keys/{}", stranger.public_key().to_hex());
        for (method, path, body) in [
            (get.clone(), "/admin/keys", &b""[..]),
            (get.clone(), "/admin/audit", &b""[..]),
            (put.clone(), mallory_path.as_str(), &grant[..]),
            (put.clone(), alice_path.as_str(), &grant[..]),
            (delete.clone(), alice_path.as_str(), &b""[..]),
        ] {
            let (status, _) = api.signed(&alice, method.clone(), path, body).await?;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        }

        // The owner sees her, and the record of granting her
        let (status, listed) = api.signed(&owner, get.clone(), "/admin/keys", b"").await?;
        assert_eq!(status, StatusCode::OK);
        let listed: AdminKeys = serde_json::from_value(listed)?;
        assert_eq!(listed.owners.len(), 1);
        assert_eq!(listed.delegates.len(), 1);
        assert_eq!(
            listed.delegates[0].key.npub,
            alice.public_key().to_bech32()?
        );
        let (status, log) = api
            .signed(&owner, get.clone(), "/admin/audit?limit=5", b"")
            .await?;
        assert_eq!(status, StatusCode::OK);
        let log: AdminAuditPage = serde_json::from_value(log)?;
        assert_eq!(log.entries.len(), 1);
        assert_eq!(log.entries[0].action, "key.grant");
        assert_eq!(log.entries[0].subject, alice.public_key().to_hex());
        assert_eq!(
            log.entries[0].subject_key.as_ref().map(|key| &key.npub),
            Some(&alice.public_key().to_bech32()?)
        );

        // Revoked, her very next request is refused
        let (status, _) = api.signed(&owner, delete.clone(), &alice_path, b"").await?;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = api
            .signed(&alice, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = api.signed(&owner, delete.clone(), &alice_path, b"").await?;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // An expired grant is refused like none
        let soon = Utc::now() + chrono::Duration::hours(1);
        let expiring = serde_json::to_vec(&json!({ "label": "Alice", "expires_at": soon }))?;
        let (status, _) = api
            .signed(&owner, put.clone(), &alice_path, &expiring)
            .await?;
        assert_eq!(status, StatusCode::OK);
        db.client
            .execute(
                "UPDATE admin_keys SET expires_at = now() - interval '1 second'",
                &[],
            )
            .await?;
        let (status, _) = api
            .signed(&alice, get.clone(), "/admin/session", b"")
            .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // API replicas refuse admins, after checking who they are, and
        // without spending the signature: the worker still takes it
        let replica = Api::new(db.observer(false).await?, &owner)?;
        let (status, _, _) = replica
            .send(get.clone(), "/admin/session", token, b"")
            .await?;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _, _) = replica
            .send(get.clone(), "/admin/session", None, b"")
            .await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let header = Signed::new(&get, "/admin/session", b"").header(&owner);
        let signed = Some(format!("Nostr {header}"));
        let (status, _, _) = replica
            .send(get.clone(), "/admin/session", signed.clone(), b"")
            .await?;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _, _) = api.send(get, "/admin/session", signed, b"").await?;
        assert_eq!(status, StatusCode::OK);
        Ok(())
    }
    .await;
    db.finish(result).await
}

/// The status and role `keys` gets from `GET /admin/session`
async fn role_of(api: &Api, keys: &Keys) -> anyhow::Result<(StatusCode, Option<String>)> {
    let (status, session) = api.signed(keys, Method::GET, "/admin/session", b"").await?;
    Ok((status, session["role"].as_str().map(str::to_owned)))
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_a_grant_outlives_owner_status_until_revoked() -> anyhow::Result<()> {
    let db = TestSchema::new("owner_grant").await?;
    let result = async {
        let (owner, alice) = (Keys::generate(), Keys::generate());
        let get = Method::GET;
        let alice_path = format!("/admin/keys/{}", alice.public_key().to_hex());
        let grant = serde_json::to_vec(&json!({ "label": "Alice" }))?;

        // A delegate first
        let before = Api::new(db.observer(true).await?, &owner)?;
        let (status, _) = before
            .signed(&owner, Method::PUT, &alice_path, &grant)
            .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            role_of(&before, &alice).await?,
            (StatusCode::OK, Some("delegate".into()))
        );

        // Then listed as an owner: an owner, whatever the grant says, and the
        // grant shows as held by an owner
        let both = Api::with_owners(db.observer(true).await?, &[&owner, &alice])?;
        assert_eq!(
            role_of(&both, &alice).await?,
            (StatusCode::OK, Some("owner".into()))
        );
        let (status, listed) = both.signed(&owner, get.clone(), "/admin/keys", b"").await?;
        assert_eq!(status, StatusCode::OK);
        let listed: AdminKeys = serde_json::from_value(listed)?;
        assert!(listed.delegates[0].also_owner);
        let observer = db.observer(true).await?;
        let auth = AdminAuth::new(
            TOKEN,
            Some(&format!(
                "{},{}",
                owner.public_key().to_hex(),
                alice.public_key().to_hex()
            )),
            Some(PUBLIC_URL),
        )?;
        assert_eq!(
            keys::owners_with_grants(&observer.connection().await?, auth.owners()).await?,
            vec![alice.public_key()]
        );

        // Removed from the owners, the grant still makes it a delegate, as
        // documented; only revoking it takes all access away
        let after = Api::new(db.observer(true).await?, &owner)?;
        assert_eq!(
            role_of(&after, &alice).await?,
            (StatusCode::OK, Some("delegate".into()))
        );
        let (status, _) = after
            .signed(&owner, Method::DELETE, &alice_path, b"")
            .await?;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(role_of(&after, &alice).await?.0, StatusCode::FORBIDDEN);
        Ok(())
    }
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires an isolated FMO_ADMIN_TEST_DATABASE"]
async fn postgres_federation_admin_routes_check_roles_and_are_not_cached() -> anyhow::Result<()> {
    let db = TestSchema::new("fed_routes").await?;
    let result = async {
        let (owner, alice) = (Keys::generate(), Keys::generate());
        let api = Api::new(db.observer(true).await?, &owner)?;
        let alice_path = format!("/admin/keys/{}", alice.public_key().to_hex());
        let grant = serde_json::to_vec(&json!({ "label": "Alice" }))?;
        let (status, _) = api.signed(&owner, Method::PUT, &alice_path, &grant).await?;
        assert_eq!(status, StatusCode::OK);

        // An id the observer doesn't observe: owners get past authentication
        // to the handler's 404 without anything reaching a guardian
        let unobserved = "00".repeat(32);
        let backfill = format!("/federations/{unobserved}/backfill");
        let reset = format!("/federations/{unobserved}/modules/mint/reset");
        let body = b"{}".to_vec();
        let add = serde_json::to_vec(&json!({ "invite": "not an invite" }))?;
        let routes = [
            (Method::POST, backfill.as_str(), &body),
            (Method::POST, reset.as_str(), &body),
            (Method::PUT, "/federations", &add),
        ];
        let audited = || async {
            let count: i64 = db
                .client
                .query_one(
                    "SELECT COUNT(*) FROM admin_audit_log WHERE action <> 'key.grant'",
                    &[],
                )
                .await?
                .get(0);
            anyhow::Ok(count)
        };

        for (method, path, body) in &routes {
            // Nobody, and a wrong token
            for authorization in [None, Some("Bearer wrong".to_owned())] {
                let (status, headers, _) = api
                    .send(method.clone(), path, authorization, body)
                    .await?;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
                assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{method} {path}");
            }
        }
        // A delegate may add federations, and nothing else
        for (method, path, body) in &routes[..2] {
            let (status, _) = api.signed(&alice, method.clone(), path, body).await?;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        }
        let (status, _) = api.signed(&alice, Method::PUT, "/federations", &add).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(audited().await?, 0, "refused requests ran nothing");

        // Owners reach the handlers, and their answers aren't cached either
        for (method, path, body) in &routes[..2] {
            let header = Signed::new(method, path, body).header(&owner);
            let (status, headers, _) = api
                .send(method.clone(), path, Some(format!("Nostr {header}")), body)
                .await?;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
            assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{method} {path}");
        }
        assert_eq!(audited().await?, 2);
        let failed: i64 = db
            .client
            .query_one(
                "SELECT COUNT(*) FROM admin_audit_log WHERE action <> 'key.grant' AND outcome = 'failed'",
                &[],
            )
            .await?
            .get(0);
        assert_eq!(failed, 2);

        // The public list shares the path of PUT /federations, but not its
        // no-store layer (this schema lacks its tables, so it answers 500;
        // the header is what matters here)
        let (_, headers, _) = api
            .send(Method::GET, "/federations", None, b"")
            .await?;
        assert_ne!(
            headers.get(header::CACHE_CONTROL).map(|value| value.as_bytes()),
            Some(&b"no-store"[..])
        );
        Ok(())
    }
    .await;
    db.finish(result).await
}
