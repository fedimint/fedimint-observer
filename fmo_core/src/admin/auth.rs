//! Who is asking: turns a request's `Authorization` header into an [`Admin`].
//!
//! Two kinds of credentials are accepted:
//!
//! - `Bearer <FO_ADMIN_AUTH>`: the shared secret, with owner rights. For
//!   scripts; it can't be revoked per person, so it isn't for delegates.
//! - `Nostr <base64 event>`: a NIP-98 event signed for this one request by an
//!   owner's key (`FO_ADMIN_NPUBS`) or a delegate's (`admin_keys`). The browser
//!   never holds a secret, and there is no session to steal or to revoke.
//!
//! A signed request is accepted only if all of this holds:
//!
//! - the event's id and signature are valid, and its kind is 27235
//! - it was signed within [`MAX_CLOCK_SKEW`] of now
//! - its one `u` tag is this request's URL under `FO_PUBLIC_API_URL`. The
//!   `Host` header is never used: whoever sends the request picks it, so a
//!   request an admin signed for another site could be replayed here.
//! - its one `method` tag is the request's method
//! - its `payload` tag is the SHA-256 of the body, whenever there is a body, so
//!   a signature can't be moved to a different body
//! - its key is an owner's or holds a current grant, looked up for every
//!   request, so revoking takes effect at once
//! - its id was not used before (`admin_auth_events`)

use std::collections::BTreeSet;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{FromRequest, OriginalUri, Request};
use axum::http::{header, HeaderMap, Method, Uri};
use base64::Engine;
use bitcoin::hashes::{sha256, Hash};
use chrono::{DateTime, Utc};
use fmo_api_types::AdminRole;
use nostr_sdk::{Event, Kind, PublicKey, Url};
use serde::de::DeserializeOwned;
use subtle::ConstantTimeEq;

use super::keys::{self, Delegate};
use crate::error::AppError;
use crate::AppState;

/// How far an event's `created_at` may be from the server's clock, either way.
/// It is also how long a captured header could be replayed if the used ids
/// were lost, so it is kept short.
pub(super) const MAX_CLOCK_SKEW: Duration = Duration::from_secs(60);

/// The longest `Authorization` value read. A NIP-98 event with its three tags
/// is well under 1 KiB of base64.
const MAX_AUTHORIZATION_BYTES: usize = 4096;

/// The largest body of an admin request: an invite code naming many guardians
const MAX_ADMIN_BODY_BYTES: usize = 64 * 1024;

/// Owners are configured by hand; more than a few is a mistake
const MAX_OWNERS: usize = 16;

/// What the frontend shows when it could not have known better
const NO_ACCESS: &str = "This Nostr key has no admin access to this observer";

/// The admin a request was authenticated as
#[derive(Debug, Clone)]
pub(crate) struct Admin {
    pub principal: Principal,
    pub role: AdminRole,
    /// A delegate's grant
    pub grant: Option<Delegate>,
}

/// What authenticated a request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Principal {
    /// The `FO_ADMIN_AUTH` bearer token
    Token,
    Key(PublicKey),
}

impl Principal {
    pub(crate) fn key(&self) -> Option<&PublicKey> {
        match self {
            Principal::Token => None,
            Principal::Key(key) => Some(key),
        }
    }
}

impl Admin {
    /// Refuses delegates: maintenance and access management are the owners'
    pub(crate) fn require_owner(&self) -> Result<(), AppError> {
        match self.role {
            AdminRole::Owner => Ok(()),
            AdminRole::Delegate => Err(AppError::forbidden("Only owners can do this")),
        }
    }
}

/// The SHA-256 of the `FO_ADMIN_AUTH` token, so comparing takes the same time
/// whatever the presented token's length or content
struct TokenDigest([u8; 32]);

impl TokenDigest {
    fn new(token: &str) -> Self {
        Self(sha256::Hash::hash(token.as_bytes()).to_byte_array())
    }

    fn verify(&self, presented: &str) -> bool {
        bool::from(self.0.ct_eq(&Self::new(presented).0))
    }
}

/// Settings of signed requests, present when `FO_PUBLIC_API_URL` is set
struct NostrAuth {
    owners: BTreeSet<PublicKey>,
    /// Where browsers reach this API, without a trailing slash
    public_url: String,
}

/// The observer's admin credentials. Holds no secret in the clear.
pub(crate) struct AdminAuth {
    token: TokenDigest,
    nostr: Option<NostrAuth>,
}

impl std::fmt::Debug for AdminAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminAuth")
            .field("token", &"<redacted>")
            .field("nostr", &self.nostr.is_some())
            .finish()
    }
}

impl AdminAuth {
    /// From `FO_ADMIN_AUTH`, `FO_ADMIN_NPUBS` and `FO_PUBLIC_API_URL`
    pub(crate) fn new(
        token: &str,
        owners: Option<&str>,
        public_url: Option<&str>,
    ) -> anyhow::Result<Self> {
        // An empty token would let `Authorization: Bearer` with nothing after
        // it in
        anyhow::ensure!(!token.is_empty(), "FO_ADMIN_AUTH must not be empty");
        let owners = parse_owners(owners.unwrap_or_default())?;
        let nostr = match public_url {
            Some(url) => Some(NostrAuth {
                owners,
                public_url: parse_public_url(url)?,
            }),
            None => {
                anyhow::ensure!(
                    owners.is_empty(),
                    "FO_ADMIN_NPUBS needs FO_PUBLIC_API_URL, the URL signed requests are checked against"
                );
                None
            }
        };
        Ok(Self {
            token: TokenDigest::new(token),
            nostr,
        })
    }

    pub(crate) fn owners(&self) -> impl Iterator<Item = &PublicKey> {
        self.nostr.iter().flat_map(|nostr| &nostr.owners)
    }

    pub(crate) fn is_owner(&self, key: &PublicKey) -> bool {
        self.owners().any(|owner| owner == key)
    }
}

/// `FO_ADMIN_NPUBS`: keys as `npub1…` or hex, separated by commas or spaces
fn parse_owners(list: &str) -> anyhow::Result<BTreeSet<PublicKey>> {
    let owners = list
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|key| !key.is_empty())
        .map(|key| {
            PublicKey::parse(key)
                .map_err(|_| anyhow::anyhow!("FO_ADMIN_NPUBS has an invalid public key: {key}"))
        })
        .collect::<anyhow::Result<BTreeSet<_>>>()?;
    anyhow::ensure!(
        owners.len() <= MAX_OWNERS,
        "FO_ADMIN_NPUBS names more than {MAX_OWNERS} owners"
    );
    Ok(owners)
}

/// `FO_PUBLIC_API_URL`: the API's base URL as browsers see it. Signatures are
/// only as private as the connection carrying them, so plain HTTP is allowed
/// for loopback development only.
fn parse_public_url(url: &str) -> anyhow::Result<String> {
    let parsed =
        Url::parse(url).map_err(|_| anyhow::anyhow!("FO_PUBLIC_API_URL is not a valid URL"))?;
    let loopback = parsed.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    anyhow::ensure!(
        parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback),
        "FO_PUBLIC_API_URL must be https, or http on a loopback address"
    );
    anyhow::ensure!(
        parsed.query().is_none()
            && parsed.fragment().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none(),
        "FO_PUBLIC_API_URL must have no query, fragment or credentials"
    );
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

/// What an `Authorization` header carries
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Credentials<'a> {
    Bearer(&'a str),
    Nostr(&'a str),
}

/// The request's credentials. Several `Authorization` headers are refused
/// rather than choosing one, since a proxy in front may have checked another.
pub(super) fn credentials(headers: &HeaderMap) -> Result<Credentials<'_>, AppError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(AppError::unauthorized_because(
            "Send one Authorization header with a bearer token or a signed Nostr event",
        ));
    };
    let malformed = || AppError::unauthorized_because("Malformed Authorization header");
    if value.len() > MAX_AUTHORIZATION_BYTES {
        return Err(malformed());
    }
    let (scheme, rest) = value
        .to_str()
        .map_err(|_| malformed())?
        .split_once(' ')
        .ok_or_else(malformed)?;
    let rest = rest.trim();
    if scheme.eq_ignore_ascii_case("bearer") {
        Ok(Credentials::Bearer(rest))
    } else if scheme.eq_ignore_ascii_case("nostr") {
        Ok(Credentials::Nostr(rest))
    } else {
        Err(malformed())
    }
}

/// A NIP-98 event that authorizes exactly the request it came with
#[derive(Debug)]
pub(super) struct SignedRequest {
    pub pubkey: PublicKey,
    pub event_id: [u8; 32],
}

/// Checks the event of an `Authorization: Nostr` header against the request.
/// The error is the reason, fit to show to the caller.
pub(super) fn verify_signed_request(
    encoded: &str,
    url: &Url,
    method: &Method,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<SignedRequest, &'static str> {
    const MALFORMED: &str = "The Authorization header holds no valid Nostr event";
    let json = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| MALFORMED)?;
    let event = serde_json::from_slice::<Event>(&json).map_err(|_| MALFORMED)?;

    if event.kind() != Kind::HttpAuth {
        return Err("The signed event is not an HTTP auth event (kind 27235)");
    }
    let created_at = i64::try_from(event.created_at().as_u64()).map_err(|_| MALFORMED)?;
    if created_at.abs_diff(now.timestamp()) > MAX_CLOCK_SKEW.as_secs() {
        return Err("The request was signed too long ago or your clock is off; try again");
    }

    let signed_url = single_tag(&event, "u")?.ok_or("The signed event names no URL")?;
    if Url::parse(signed_url).ok().as_ref() != Some(url) {
        return Err("The request was signed for a different URL");
    }
    let signed_method =
        single_tag(&event, "method")?.ok_or("The signed event names no HTTP method")?;
    if !signed_method.eq_ignore_ascii_case(method.as_str()) {
        return Err("The request was signed for a different HTTP method");
    }
    let payload = single_tag(&event, "payload")?;
    // A body always needs its hash signed. Without a body the tag is optional,
    // and a tag that is there must still match.
    if !body.is_empty() || payload.is_some() {
        let hash = sha256::Hash::hash(body).to_string();
        if !payload.is_some_and(|payload| payload.eq_ignore_ascii_case(&hash)) {
            return Err("The request body does not match the signed payload hash");
        }
    }

    // Last, as the costly check: both the id (the hash of what was checked
    // above) and the signature over it
    event
        .verify()
        .map_err(|_| "The event's signature is invalid")?;

    Ok(SignedRequest {
        pubkey: event.author(),
        event_id: event.id().to_bytes(),
    })
}

/// The value of the event's tag `name`. A tag given twice is refused: a
/// signer could have shown its user one value and the server read the other.
fn single_tag<'a>(event: &'a Event, name: &str) -> Result<Option<&'a str>, &'static str> {
    let mut values = event.tags().iter().filter_map(|tag| match tag.as_vec() {
        [tag_name, value, ..] if tag_name == name => Some(value.as_str()),
        _ => None,
    });
    match (values.next(), values.next()) {
        (value, None) => Ok(value),
        _ => Err("The signed event repeats a tag"),
    }
}

impl NostrAuth {
    /// The URL a request to `uri` must have been signed for
    fn request_url(&self, uri: &Uri) -> Option<Url> {
        let path_and_query = uri.path_and_query()?.as_str();
        Url::parse(&format!("{}{path_and_query}", self.public_url)).ok()
    }
}

/// Authenticates a request and returns its body, read only once the header
/// looks like credentials
async fn authenticate(request: Request, state: &AppState) -> Result<(Admin, Bytes), AppError> {
    let (parts, body) = request.into_parts();
    let credentials = credentials(&parts.headers)?;
    let body = axum::body::to_bytes(body, MAX_ADMIN_BODY_BYTES)
        .await
        .map_err(|_| AppError::payload_too_large("The request body is too large"))?;
    let observer = &state.federation_observer;

    let admin = match credentials {
        Credentials::Bearer(token) => {
            if !state.admin.token.verify(token) {
                return Err(AppError::unauthorized());
            }
            Admin {
                principal: Principal::Token,
                role: AdminRole::Owner,
                grant: None,
            }
        }
        Credentials::Nostr(encoded) => {
            let Some(nostr) = &state.admin.nostr else {
                return Err(AppError::unauthorized_because(
                    "Signing in with Nostr is not enabled on this observer",
                ));
            };
            // Nested routers strip their prefix from the URI; the client
            // signed the whole path
            let uri = parts
                .extensions
                .get::<OriginalUri>()
                .map_or(&parts.uri, |original| &original.0);
            let url = nostr
                .request_url(uri)
                .ok_or_else(|| AppError::bad_request("Invalid request URL"))?;
            let now = Utc::now();
            let signed = verify_signed_request(encoded, &url, &parts.method, &body, now)
                .map_err(AppError::unauthorized_because)?;

            let connection = observer.connection().await?;
            let (role, grant) = if nostr.owners.contains(&signed.pubkey) {
                (AdminRole::Owner, None)
            } else {
                let grant = keys::active_delegate(&connection, &signed.pubkey, now)
                    .await?
                    .ok_or_else(|| AppError::forbidden(NO_ACCESS))?;
                (AdminRole::Delegate, Some(grant))
            };
            // Only requests of admin keys are remembered, so strangers can't
            // make the observer write
            if !keys::record_signed_request(&connection, &signed.event_id, now).await? {
                return Err(AppError::unauthorized_because(
                    "This signed request was already used; sign a new one",
                ));
            }
            Admin {
                principal: Principal::Key(signed.pubkey),
                role,
                grant,
            }
        }
    };

    // After the credentials, so other callers learn nothing about the
    // deployment
    if !observer.is_worker() {
        return Err(AppError::unavailable(
            "Administrative operations must be sent to the worker instance",
        ));
    }
    Ok((admin, body))
}

/// An admin request without a body
#[async_trait]
impl FromRequest<AppState> for Admin {
    type Rejection = AppError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(authenticate(request, state).await?.0)
    }
}

/// An admin request with a JSON body. The body is parsed only after the
/// request is authenticated, so other callers learn nothing from parse errors.
pub(crate) struct AdminJson<T>(pub Admin, pub T);

#[async_trait]
impl<T: DeserializeOwned> FromRequest<AppState> for AdminJson<T> {
    type Rejection = AppError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let (admin, body) = authenticate(request, state).await?;
        let value = serde_json::from_slice(&body)
            .map_err(|_| AppError::bad_request("The request body is not the expected JSON"))?;
        Ok(Self(admin, value))
    }
}
