use std::str::FromStr;

use fedimint_api_client::api::FederationApiExt;
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
    let api = guardian_api(&connectors, &peers, None);
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
