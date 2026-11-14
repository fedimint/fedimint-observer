//! The one place this module reads the Lightning v1 module's tables, to link
//! v1 and v2 registrations of the same node. Core creates `fmo_ln.gateways`
//! whatever modules are built, but without the Lightning v1 module nothing
//! updates it, so its rows only age out.
use std::str::FromStr;

use fedimint_core::secp256k1::PublicKey;

/// A federation's Lightning v1 gateways that aren't retired (`$1`
/// federation): announced within the last 7 days, as
/// `GATEWAY_RETIRED_AFTER_DAYS` in `fmo_module_ln`.
pub(crate) const LNV1_GATEWAYS_SQL: &str =
    "SELECT gateway_id,node_pub_key,api_endpoint,first_seen,last_seen FROM fmo_ln.gateways
     WHERE federation_id=$1 AND last_seen>=now()-interval '7 days'
     ORDER BY gateway_id";

/// A node key as both protocols store it, compressed lowercase hex; `None`
/// if it isn't a public key
pub(crate) fn node_key(key: &str) -> Option<String> {
    PublicKey::from_str(key).ok().map(|key| key.to_string())
}
