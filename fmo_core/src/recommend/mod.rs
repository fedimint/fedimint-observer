//! Recommendations: what the community says about federations, independent
//! of what the observer measures. Federation announcements (Nostr kind 38173)
//! and recommendations with star ratings (kind 38000) are synced from Nostr
//! relays, verified and stored; this layer serves them, publishes events
//! users submit, and gives the federations list each federation's rating.
//!
//! Collection (the relay sync), storage (`nostr_*` tables) and the endpoints
//! live together here because nothing else reads Nostr data: the rest of core
//! only calls [`FederationObserver::federation_ratings`].
//!
//! [`FederationObserver::federation_ratings`]: crate::federation::observer::FederationObserver

mod nostr;

use axum::routing::{get, put};
use axum::Router;

use crate::AppState;

/// Endpoints at the root: `/nostr/federations` lists announced federations
/// (GET) and publishes an announcement (PUT)
pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(
        "/nostr/federations",
        get(nostr::get_nostr_federations).put(nostr::publish_federation_event),
    )
}

/// Endpoints under `/federations`: `/nostr/rating` publishes a rating (PUT)
pub(crate) fn federations_routes() -> Router<AppState> {
    Router::new().route("/nostr/rating", put(nostr::publish_rating_event))
}
