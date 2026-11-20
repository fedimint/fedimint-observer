use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use fedimint_api_client::api::FederationApiExt;
use fedimint_connectors::{IConnection, IGuardianConnection};
use fedimint_core::endpoint_constants::STATUS_ENDPOINT;

use super::*;

#[test]
fn reconnect_delay_doubles_up_to_its_cap() {
    let delays = (1..=8).map(retry_delay).collect::<Vec<_>>();
    assert_eq!(delays[0], MIN_RETRY_DELAY);
    assert_eq!(delays[1], MIN_RETRY_DELAY * 2);
    assert!(delays.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(delays[7], MAX_RETRY_DELAY);
}

#[tokio::test]
async fn failed_connection_backs_off_instead_of_redialing() -> anyhow::Result<()> {
    let connectors = ConnectorRegistry::build_from_testing_defaults()
        .bind()
        .await?;
    // Nothing listens on port 1.
    let peers = BTreeMap::from([(PeerId::from(0), SafeUrl::from_str("ws://127.0.0.1:1/")?)]);
    let api = guardian_api(
        &connectors,
        &GuardianConnectionRegistry::default(),
        &peers,
        None,
        ConnectionClass::Trusted,
    );
    let request = || {
        api.request_single_peer::<Value>(
            STATUS_ENDPOINT.to_owned(),
            ApiRequestErased::default(),
            PeerId::from(0),
        )
    };

    let first = request().await.expect_err("nothing accepts the connection");
    assert!(!format!("{first}").contains("Waiting to reconnect"));
    let started = Instant::now();
    let second = request().await.expect_err("the peer is backing off");
    assert!(format!("{second}").contains("Waiting to reconnect"));
    assert!(started.elapsed() < Duration::from_millis(100), "no redial");

    let unknown = api
        .request_single_peer::<Value>(
            STATUS_ENDPOINT.to_owned(),
            ApiRequestErased::default(),
            PeerId::from(9),
        )
        .await
        .expect_err("peer 9 is not in the client");
    assert!(format!("{unknown}").contains("Invalid peer id"));
    Ok(())
}

#[tokio::test]
async fn canceled_dial_still_starts_the_retry_delay() {
    let peer = PeerConnection::default();
    tokio::time::timeout(
        Duration::from_millis(10),
        peer.connection(futures::future::pending::<ServerResult<DynGuaridianConnection>>),
    )
    .await
    .expect_err("the caller deadline cancels the dial");

    let error = peer
        .connection(|| async { panic!("backoff must prevent a second dial") })
        .await
        .expect_err("the canceled attempt is backing off");
    assert!(format!("{error}").contains("Waiting to reconnect"));
}

#[test]
fn duplicate_peer_urls_share_state_only_with_matching_credentials() -> anyhow::Result<()> {
    let registry = GuardianConnectionRegistry::with_limits(3, 0, 0);
    let url = SafeUrl::from_str("wss://guardian.example/")?;
    let first = registry.connection(&url, Some("secret"), ConnectionClass::Trusted)?;
    let duplicate = registry.connection(&url, Some("secret"), ConnectionClass::Trusted)?;
    let other_credentials = registry.connection(&url, Some("other"), ConnectionClass::Trusted)?;

    assert!(Arc::ptr_eq(&first, &duplicate));
    assert!(!Arc::ptr_eq(&first, &other_credentials));
    Ok(())
}

#[test]
fn announced_lookup_exhaustion_preserves_literal_fallback_capacity() -> anyhow::Result<()> {
    let registry = GuardianConnectionRegistry::with_limits(0, 1, 1);
    let announced = SafeUrl::from_str("wss://announced.example/")?;
    registry.connection(&announced, None, ConnectionClass::Lookup)?;

    let another_announced = SafeUrl::from_str("wss://another-announced.example/")?;
    assert!(registry
        .connection(&another_announced, None, ConnectionClass::Lookup)
        .is_err());
    let literal = SafeUrl::from_str("wss://literal.example/")?;
    registry.connection(&literal, None, ConnectionClass::LookupFallback)?;
    Ok(())
}

#[derive(Debug)]
struct CountingConnection(Arc<AtomicUsize>);

impl Drop for CountingConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl IConnection for CountingConnection {
    fn is_connected(&self) -> bool {
        true
    }

    async fn await_disconnection(&self) {
        futures::future::pending().await
    }
}

#[async_trait::async_trait]
impl IGuardianConnection for CountingConnection {
    async fn request(
        &self,
        _method: fedimint_core::module::ApiMethod,
        _request: ApiRequestErased,
    ) -> ServerResult<Value> {
        Ok(Value::Null)
    }
}

#[tokio::test]
async fn successful_rotations_stop_at_the_connection_budget() -> anyhow::Result<()> {
    let registry = GuardianConnectionRegistry::with_limits(2, 0, 0);
    let live = Arc::new(AtomicUsize::new(0));

    for port in [1001, 1002] {
        let url = SafeUrl::from_str(&format!("ws://127.0.0.1:{port}/"))?;
        let peer = registry.connection(&url, None, ConnectionClass::Trusted)?;
        let live_for_dial = live.clone();
        let connection = peer
            .connection(|| async move {
                live_for_dial.fetch_add(1, Ordering::SeqCst);
                Ok(CountingConnection(live_for_dial).into_dyn())
            })
            .await?;
        drop(connection);
        drop(peer); // Simulate retiring the route client after a rotation.
    }

    assert_eq!(live.load(Ordering::SeqCst), 2);
    let next = SafeUrl::from_str("ws://127.0.0.1:1003/")?;
    let error = registry
        .connection(&next, None, ConnectionClass::Trusted)
        .expect_err("a third successful endpoint could outlive its retired client");
    assert!(format!("{error}").contains("connection budget exhausted"));
    assert_eq!(live.load(Ordering::SeqCst), 2);
    Ok(())
}
