-- Incrementally maintained per-federation totals
BEGIN;

INSERT INTO schema_version (version)
VALUES (9);

-- Wait for in-flight writers to finish and block new ones, so that no rows can slip
-- through between the seed below and the triggers becoming visible to other backends.
LOCK TABLE transactions, transaction_inputs IN SHARE MODE;

-- Deliberately has no secondary indexes: the table holds one row per federation but is
-- updated once per inserted transaction/input, so we want updates to stay HOT.
CREATE TABLE IF NOT EXISTS federation_totals (
    federation_id BYTEA PRIMARY KEY REFERENCES federations (federation_id),
    tx_count BIGINT NOT NULL DEFAULT 0,
    tx_volume_msat BIGINT NOT NULL DEFAULT 0
);

-- One dead tuple per inserted transaction/input, on a table of a handful of rows. Without
-- this the default scale factor (20% of a ~17 row table) would never trigger a vacuum in
-- time and the table would bloat far out of proportion to its logical size.
ALTER TABLE federation_totals
    SET (
        autovacuum_vacuum_scale_factor = 0.0,
        autovacuum_vacuum_threshold = 100,
        autovacuum_analyze_scale_factor = 0.0,
        autovacuum_analyze_threshold = 100
    );

CREATE OR REPLACE FUNCTION federation_totals_count_tx() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO federation_totals (federation_id, tx_count)
    VALUES (NEW.federation_id, 1)
    ON CONFLICT (federation_id) DO UPDATE
        SET tx_count = federation_totals.tx_count + 1;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- amount_msat is NULL for input kinds other than ln/mint/wallet. SUM() skips NULLs, so
-- coalesce to 0 to match the aggregate this replaces.
CREATE OR REPLACE FUNCTION federation_totals_add_volume() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO federation_totals (federation_id, tx_volume_msat)
    VALUES (NEW.federation_id, COALESCE(NEW.amount_msat, 0))
    ON CONFLICT (federation_id) DO UPDATE
        SET tx_volume_msat = federation_totals.tx_volume_msat + COALESCE(NEW.amount_msat, 0);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Row level AFTER INSERT triggers do not fire for rows skipped by ON CONFLICT DO NOTHING,
-- which is what keeps these counters correct when a session gets reprocessed.
CREATE TRIGGER transactions_bump_totals
    AFTER INSERT ON transactions
    FOR EACH ROW EXECUTE FUNCTION federation_totals_count_tx();

CREATE TRIGGER transaction_inputs_bump_totals
    AFTER INSERT ON transaction_inputs
    FOR EACH ROW EXECUTE FUNCTION federation_totals_add_volume();

INSERT INTO federation_totals (federation_id, tx_count, tx_volume_msat)
SELECT f.federation_id,
       (SELECT COUNT(*)
        FROM transactions t
        WHERE t.federation_id = f.federation_id),
       (SELECT COALESCE(SUM(ti.amount_msat), 0)
        FROM transaction_inputs ti
        WHERE ti.federation_id = f.federation_id)
FROM federations f
ON CONFLICT (federation_id) DO UPDATE
    SET tx_count = EXCLUDED.tx_count,
        tx_volume_msat = EXCLUDED.tx_volume_msat;

COMMIT;
