SELECT g.api_endpoint,g.announcing_guardians,g.registered,g.first_seen,g.last_seen,
       g.routing_info,g.routing_observed_at,g.probed_at,g.reachable,g.federation_supported,g.latency_ms,
       health.buckets,activity.metrics,keys.claimed_keys,keys.contested_keys
FROM fmo_lnv2.gateways g
-- Module keys are self-reported by each endpoint and unverified. A key that
-- another registered endpoint also claims is contested and credits neither.
LEFT JOIN LATERAL (
    SELECT COUNT(*) AS claimed_keys,COUNT(*) FILTER(WHERE ck.contested) AS contested_keys,
           COALESCE(array_agg(ck.module_public_key) FILTER(WHERE NOT ck.contested),'{}') AS attributed
    FROM (
        SELECT k.module_public_key,EXISTS(
            SELECT 1 FROM fmo_lnv2.gateway_keys o
            JOIN fmo_lnv2.gateways og ON og.federation_id=o.federation_id
              AND og.module_instance_id=o.module_instance_id AND og.api_endpoint=o.api_endpoint
            WHERE o.federation_id=k.federation_id AND o.module_instance_id=k.module_instance_id
              AND o.module_public_key=k.module_public_key AND o.api_endpoint<>k.api_endpoint AND og.registered
        ) AS contested
        FROM fmo_lnv2.gateway_keys k
        WHERE k.federation_id=g.federation_id AND k.module_instance_id=g.module_instance_id AND k.api_endpoint=g.api_endpoint
    ) ck
) keys ON true
LEFT JOIN LATERAL (
    SELECT COALESCE(jsonb_agg(to_jsonb(b)),'[]'::jsonb) AS buckets FROM (
        SELECT LEAST($7::integer-1,GREATEST(0,floor(extract(epoch FROM (h.hour-$5::timestamptz))/$6::double precision)::integer)) AS bucket,
            SUM(samples)::bigint AS samples,SUM(online_samples)::bigint AS online_samples,
            SUM(latency_sum_ms)::bigint AS latency_sum_ms,SUM(latency_samples)::bigint AS latency_samples
        FROM fmo_lnv2.gateway_health_hourly h
        WHERE h.federation_id=g.federation_id AND h.module_instance_id=g.module_instance_id
          AND h.api_endpoint=g.api_endpoint AND h.hour>=$5
        GROUP BY bucket
    ) b
) health ON true
LEFT JOIN LATERAL (
    SELECT jsonb_build_object(
        'incoming_contracts',COUNT(*) FILTER(WHERE c.direction='incoming'),
        'outgoing_contracts',COUNT(*) FILTER(WHERE c.direction='outgoing'),
        'incoming_msat',COALESCE(SUM(c.amount_msat) FILTER(WHERE c.direction='incoming'),0),
        'outgoing_msat',COALESCE(SUM(c.amount_msat) FILTER(WHERE c.direction='outgoing'),0),
        'outgoing_claimed',COUNT(*) FILTER(WHERE c.direction='outgoing' AND r.outcome='claimed'),
        'outgoing_refunded',COUNT(*) FILTER(WHERE c.direction='outgoing' AND r.outcome='refunded'),
        'outgoing_cancelled',COUNT(*) FILTER(WHERE c.direction='outgoing' AND r.outcome='cancelled')
    ) AS metrics
    FROM fmo_lnv2.contract_activity c
    JOIN session_times st ON st.federation_id=c.federation_id AND st.session_index=c.session_index
    LEFT JOIN fmo_lnv2.contract_resolutions r ON r.federation_id=c.federation_id
      AND r.module_instance_id=c.module_instance_id AND r.funding_txid=c.txid AND r.funding_out_index=c.out_index
    WHERE c.federation_id=g.federation_id AND c.module_instance_id=g.module_instance_id
      AND c.gateway_key=ANY(keys.attributed)
      AND st.estimated_session_timestamp>=$5 AT TIME ZONE 'UTC'
) activity ON true
WHERE g.federation_id=$1 AND g.module_instance_id=$2 AND ($3::text IS NULL OR g.api_endpoint>$3)
ORDER BY g.api_endpoint LIMIT $4
