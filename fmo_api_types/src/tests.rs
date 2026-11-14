use super::*;

/// `/config/:invite/gateways` is a stable API: it sets only these fields,
/// and the unset ones must stay out of the JSON
#[test]
fn config_gateway_info_json_is_unchanged() {
    let info = GatewayInfo {
        gateway_id: "gateway".into(),
        node_pub_key: "node".into(),
        lightning_alias: "alias".into(),
        api_endpoint: "https://gateway.example/v1/".into(),
        vetted: true,
        raw: Some(serde_json::json!({"info": {}})),
        first_seen: None,
        last_seen: None,
        activity_7d: None,
        activity_window: None,
        uptime_window: None,
        metrics_window: None,
        status: None,
        coverage_pct: None,
        estimated_uptime_pct: None,
        estimated_online_minutes: None,
        estimated_offline_minutes: None,
        estimated_unknown_minutes: None,
    };
    assert_eq!(
        serde_json::to_value(info).unwrap(),
        serde_json::json!({
            "gateway_id": "gateway",
            "node_pub_key": "node",
            "lightning_alias": "alias",
            "api_endpoint": "https://gateway.example/v1/",
            "vetted": true,
            "raw": {"info": {}},
        })
    );
}
