//! Administration: who may change what the observer does, and the record of
//! what they did.
//!
//! - **Owners** run the observer. They are the holders of the `FO_ADMIN_AUTH`
//!   bearer token and of the Nostr keys in `FO_ADMIN_NPUBS`. Configured, never
//!   stored, so nothing in the database can add or remove one.
//! - **Delegates** are Nostr keys an owner granted access to (`admin_keys`).
//!   They can add federations and nothing else, a limited number per day. An
//!   owner revokes a grant at any time, and it stops working with the
//!   delegate's next request.
//!
//! Files: `auth.rs` authenticates a request ([`Admin`], [`AdminJson`]),
//! `keys.rs` stores grants and used request ids, `audit.rs` records actions.
//! Admin handlers elsewhere (`crate::federation`) take one of the two
//! extractors, so a handler can't run before its caller is authenticated.
//!
//! Endpoints, all `Cache-Control: no-store`:
//!
//! - `GET /admin/session`: who the caller is
//! - `GET /admin/keys`: owners and grants (owners only)
//! - `PUT /admin/keys/:pubkey`, `DELETE /admin/keys/:pubkey`: grant and revoke
//!   (owners only)
//! - `GET /admin/audit`: the audit log, newest first (owners only)

mod audit;
mod auth;
mod keys;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fmo_api_types::{
    AdminAuditPage, AdminDelegate, AdminKeyId, AdminKeys, AdminRole, AdminSession, GrantAdminKey,
};
use nostr_sdk::{PublicKey, ToBech32};

pub(crate) use self::audit::{audited, Action};
pub(crate) use self::auth::{Admin, AdminAuth, AdminJson};
use self::keys::Delegate;
use crate::error::{AppError, ClientError};
use crate::pagination::Page;
use crate::AppState;

/// The longest a grant can be set to last. Longer-lived access is granted
/// without an expiry, on purpose rather than by a date nobody will remember.
const MAX_GRANT_DAYS: i64 = 366;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/session", get(session))
        .route("/admin/keys", get(list_keys))
        .route("/admin/keys/:pubkey", put(grant_key).delete(revoke_key))
        .route("/admin/audit", get(audit_log))
        .layer(axum::middleware::map_response(no_store))
}

/// Admin answers differ by caller while their URLs don't, so no cache between
/// the observer and the admin may keep one
async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A key in both spellings: hex for the API, `npub1…` for people
pub(crate) fn key_id(key: &PublicKey) -> AdminKeyId {
    AdminKeyId {
        pubkey: key.to_hex(),
        // Encoding 32 bytes can't fail; an empty npub only loses the spelling
        npub: key.to_bech32().unwrap_or_default(),
    }
}

fn delegate_view(delegate: &Delegate, now: DateTime<Utc>) -> AdminDelegate {
    AdminDelegate {
        key: key_id(&delegate.pubkey),
        label: delegate.label.clone(),
        granted_by: delegate.granted_by.as_ref().map(key_id),
        granted_at: delegate.granted_at,
        expires_at: delegate.expires_at,
        expired: delegate.expired(now),
    }
}

/// Who the caller is, for the frontend to show what they can do
async fn session(admin: Admin) -> Json<AdminSession> {
    Json(AdminSession {
        role: admin.role,
        key: admin.principal.key().map(key_id),
        label: admin.grant.as_ref().map(|grant| grant.label.clone()),
        expires_at: admin.grant.as_ref().and_then(|grant| grant.expires_at),
    })
}

async fn list_keys(
    State(state): State<AppState>,
    admin: Admin,
) -> crate::error::Result<Json<AdminKeys>> {
    admin.require_owner()?;
    let connection = state.federation_observer.connection().await?;
    let now = Utc::now();
    Ok(Json(AdminKeys {
        owners: state.admin.owners().map(key_id).collect(),
        delegates: keys::list_delegates(&connection)
            .await?
            .iter()
            .map(|delegate| delegate_view(delegate, now))
            .collect(),
        max_delegates: keys::MAX_DELEGATES as u32,
    }))
}

/// A key from a path, as `npub1…` or hex
fn parse_key(key: &str) -> Result<PublicKey, AppError> {
    PublicKey::parse(key)
        .map_err(|_| AppError::bad_request("Not a Nostr public key (npub1… or 64 hex characters)"))
}

async fn grant_key(
    Path(pubkey): Path<String>,
    State(state): State<AppState>,
    AdminJson(admin, body): AdminJson<GrantAdminKey>,
) -> crate::error::Result<Json<AdminDelegate>> {
    admin.require_owner()?;
    let pubkey = parse_key(&pubkey)?;
    if state.admin.is_owner(&pubkey) {
        return Err(AppError::bad_request("This key is an owner already"));
    }
    let now = Utc::now();
    if let Some(expires_at) = body.expires_at {
        if expires_at <= now || expires_at > now + chrono::Duration::days(MAX_GRANT_DAYS) {
            return Err(AppError::bad_request(format!(
                "The expiry must be within the next {MAX_GRANT_DAYS} days; leave it out for access that doesn't expire"
            )));
        }
    }
    let mut connection = state.federation_observer.connection().await?;
    let delegate = keys::grant(
        &mut connection,
        &admin,
        &pubkey,
        &body.label,
        body.expires_at,
    )
    .await?;
    Ok(Json(delegate_view(&delegate, now)))
}

async fn revoke_key(
    Path(pubkey): Path<String>,
    State(state): State<AppState>,
    admin: Admin,
) -> crate::error::Result<StatusCode> {
    admin.require_owner()?;
    let pubkey = parse_key(&pubkey)?;
    let mut connection = state.federation_observer.connection().await?;
    if !keys::revoke(&mut connection, &admin, &pubkey).await? {
        return Err(ClientError::not_found("This key holds no access").into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `after` continues a listing past the entry with that id, towards older ones
async fn audit_log(
    Query(page): Query<Page<i64>>,
    State(state): State<AppState>,
    admin: Admin,
) -> crate::error::Result<Json<AdminAuditPage>> {
    admin.require_owner()?;
    let connection = state.federation_observer.connection().await?;
    let (entries, next) = audit::list(&connection, page.after, page.limit()?).await?;
    Ok(Json(AdminAuditPage { entries, next }))
}

/// For [`AdminRole`] in messages and logs
pub(crate) fn role_name(role: AdminRole) -> &'static str {
    match role {
        AdminRole::Owner => "owner",
        AdminRole::Delegate => "delegate",
    }
}

#[cfg(test)]
mod tests;
