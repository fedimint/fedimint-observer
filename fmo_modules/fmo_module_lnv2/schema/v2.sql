-- Gateway module keys are self-reported in routing info and unverified. Track
-- when each was last claimed, so an endpoint keeps a bounded set of recent keys,
-- and index by key, so a key claimed by several endpoints is found as contested.
ALTER TABLE fmo_lnv2.gateway_keys
    ADD COLUMN last_claimed TIMESTAMPTZ NOT NULL DEFAULT now();
CREATE INDEX gateway_keys_claimants ON fmo_lnv2.gateway_keys
    (federation_id, module_instance_id, module_public_key);

-- Existing rows predate the limits that writes now enforce. The numbers below
-- mirror probes::MAX_KEYS_PER_ENDPOINT, gateways::MAX_ACTIVE_GATEWAYS and
-- gateways::MAX_RETIRED_GATEWAYS, and the orders match the runtime prunes.

-- Keys: an endpoint's current key, from its latest routing info, ranks first;
-- its other keys keep their order by key, as the runtime prune breaks ties.
UPDATE fmo_lnv2.gateway_keys k SET last_claimed = now() - interval '1 second'
FROM fmo_lnv2.gateways g
WHERE g.federation_id = k.federation_id AND g.module_instance_id = k.module_instance_id
  AND g.api_endpoint = k.api_endpoint
  AND (g.routing_info->>'module_public_key') IS DISTINCT FROM k.module_public_key;
DELETE FROM fmo_lnv2.gateway_keys k USING (
    SELECT federation_id, module_instance_id, api_endpoint, module_public_key,
           row_number() OVER (PARTITION BY federation_id, module_instance_id, api_endpoint
                              ORDER BY last_claimed DESC, module_public_key) AS rank
    FROM fmo_lnv2.gateway_keys
) r
WHERE k.federation_id = r.federation_id AND k.module_instance_id = r.module_instance_id
  AND k.api_endpoint = r.api_endpoint AND k.module_public_key = r.module_public_key
  AND r.rank > 8;

-- Active registry: keep the endpoints with the strongest guardian agreement,
-- then the most recently announced; retire the rest.
UPDATE fmo_lnv2.gateways g SET registered = false FROM (
    SELECT federation_id, module_instance_id, api_endpoint,
           row_number() OVER (PARTITION BY federation_id, module_instance_id
                              ORDER BY jsonb_array_length(announcing_guardians) DESC,
                                       last_seen DESC, api_endpoint) AS rank
    FROM fmo_lnv2.gateways WHERE registered
) r
WHERE g.federation_id = r.federation_id AND g.module_instance_id = r.module_instance_id
  AND g.api_endpoint = r.api_endpoint AND r.rank > 1024;

-- Retired history, including endpoints retired above: keep the most recent.
DELETE FROM fmo_lnv2.gateways g USING (
    SELECT federation_id, module_instance_id, api_endpoint,
           row_number() OVER (PARTITION BY federation_id, module_instance_id
                              ORDER BY last_seen DESC, api_endpoint) AS rank
    FROM fmo_lnv2.gateways WHERE NOT registered
) r
WHERE g.federation_id = r.federation_id AND g.module_instance_id = r.module_instance_id
  AND g.api_endpoint = r.api_endpoint AND r.rank > 1024;

-- Aliases are self-reported and were stored at any length up to the probe
-- response limit; probes::MAX_ALIAS_CHARS now bounds what is stored and served.
UPDATE fmo_lnv2.gateways
SET routing_info = jsonb_set(routing_info, '{lightning_alias}',
    to_jsonb(left(regexp_replace(routing_info->>'lightning_alias', '[[:cntrl:]]', '', 'g'), 64)))
WHERE jsonb_typeof(routing_info->'lightning_alias') = 'string'
  AND (length(routing_info->>'lightning_alias') > 64
       OR routing_info->>'lightning_alias' ~ '[[:cntrl:]]');
