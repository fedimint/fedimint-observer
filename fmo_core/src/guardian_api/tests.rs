use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Debug)]
struct FakeConnection;

#[async_trait::async_trait]
impl fedimint_connectors::IConnection for FakeConnection {
    fn is_connected(&self) -> bool {
        true
    }

    async fn await_disconnection(&self) {
        futures::future::pending::<()>().await;
    }
}

#[async_trait::async_trait]
impl IGuardianConnection for FakeConnection {
    async fn request(&self, _: ApiMethod, _: ApiRequestErased) -> ServerResult<Value> {
        Ok(Value::Null)
    }
}

fn peers(urls: &[&str]) -> ApiPeers {
    urls.iter()
        .enumerate()
        .map(|(peer, url)| {
            (
                PeerId::from(peer as u16),
                SafeUrl::from_str(url).expect("valid test URL"),
            )
        })
        .collect()
}

#[tokio::test]
async fn unchanged_guardians_share_one_pool_entry() -> anyhow::Result<()> {
    let connectors = ConnectorRegistry::build_from_testing_defaults()
        .bind()
        .await?;
    let scope = ScopedPool::new(&connectors, None);
    let before = peers(&["ws://g0-old/", "ws://g1/", "ws://g2/", "ws://g3/"]);
    let after = peers(&["ws://g0-new/", "ws://g1/", "ws://g2/", "ws://g3/"]);
    let dials = Arc::new(AtomicUsize::new(0));

    for route_set in [&before, &after] {
        for url in route_set.values() {
            let dials = dials.clone();
            connection_with(&scope, url, move |_, _, _| {
                let dials = dials.clone();
                async move {
                    dials.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(FakeConnection) as DynGuaridianConnection)
                }
            })
            .await?;
        }
    }

    assert_eq!(dials.load(Ordering::SeqCst), before.len() + 1);
    Ok(())
}

#[tokio::test]
async fn scoped_pool_debug_redacts_secret() -> anyhow::Result<()> {
    let connectors = ConnectorRegistry::build_from_testing_defaults()
        .bind()
        .await?;
    let scope = ScopedPool::new(&connectors, Some("hunter2".to_owned()));
    let api = PooledApi {
        scope: scope.clone(),
        peers: Arc::new(peers(&["ws://g0/"])),
        peer_ids: Arc::new(BTreeSet::from([PeerId::from(0)])),
        module_id: None,
    };
    for debug in [format!("{scope:?}"), format!("{api:?}")] {
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }
    Ok(())
}
