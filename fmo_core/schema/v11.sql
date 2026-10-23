-- Linked peg-outs to on-chain transactions via recipient addresses, which are
-- dropped below. Must happen before the txid rewrite: the rewrite leaves
-- deferred foreign key checks pending, and those block ALTER TABLE.
ALTER TABLE wallet_withdrawal_transactions DROP COLUMN federation_txid;

-- Bitcoin txids use their raw hash bytes everywhere. Older withdrawal tables
-- stored display-order bytes, unlike deposits. Reverse only the legacy tables.
-- Foreign keys onto the withdrawal transactions must be deferred so txids can
-- be rewritten in place. Databases restored from dumps may name them
-- differently or lack them entirely, so look them up instead of naming them.
DO $$
DECLARE
    fk RECORD;
BEGIN
    FOR fk IN
        SELECT conrelid::regclass AS referencing_table, conname
        FROM pg_constraint
        WHERE contype = 'f'
          AND confrelid = 'wallet_withdrawal_transactions'::regclass
    LOOP
        EXECUTE format(
            'ALTER TABLE %s ALTER CONSTRAINT %I DEFERRABLE INITIALLY DEFERRED',
            fk.referencing_table, fk.conname
        );
    END LOOP;
END
$$;
CREATE FUNCTION pg_temp.reverse_txid(value BYTEA) RETURNS BYTEA LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT decode(string_agg(substr(encode(value, 'hex'), i * 2 + 1, 2), '' ORDER BY i DESC), 'hex')
    FROM generate_series(0, octet_length(value) - 1) AS i
$$;

UPDATE wallet_withdrawal_transactions SET on_chain_txid = pg_temp.reverse_txid(on_chain_txid);
UPDATE wallet_withdrawal_signatures SET on_chain_txid = pg_temp.reverse_txid(on_chain_txid);
UPDATE wallet_withdrawal_transaction_inputs SET
    on_chain_txid = pg_temp.reverse_txid(on_chain_txid),
    previous_output_txid = pg_temp.reverse_txid(previous_output_txid);
UPDATE wallet_withdrawal_transaction_outputs SET on_chain_txid = pg_temp.reverse_txid(on_chain_txid);

DROP MATERIALIZED VIEW utxos;
CREATE MATERIALIZED VIEW utxos AS
WITH candidates AS (
    SELECT on_chain_txid, on_chain_vout, address, amount_msat, federation_id, 0 AS ownership_priority FROM wallet_peg_ins
    UNION ALL
    SELECT o.on_chain_txid, o.on_chain_vout, o.address, o.amount_msat, t.federation_id, 1 AS ownership_priority
    FROM wallet_withdrawal_transaction_outputs o
    JOIN wallet_withdrawal_transactions t USING (on_chain_txid)
    -- Fedimint builds every peg-out as [recipient, change], the same layout
    -- guardians assume in their wallet summary. Other layouts stay unclassified.
    WHERE o.on_chain_vout = 1
      AND NOT EXISTS (
          SELECT 1 FROM wallet_withdrawal_transaction_outputs x
          WHERE x.on_chain_txid = o.on_chain_txid AND x.on_chain_vout > 1
      )
)
SELECT DISTINCT ON (c.on_chain_txid, c.on_chain_vout)
    c.on_chain_txid, c.on_chain_vout, c.address, c.amount_msat, c.federation_id
FROM candidates c
WHERE NOT EXISTS (
    SELECT 1 FROM wallet_withdrawal_transaction_inputs i
    WHERE i.previous_output_txid=c.on_chain_txid AND i.previous_output_vout=c.on_chain_vout
)
ORDER BY c.on_chain_txid, c.on_chain_vout, c.ownership_priority;
CREATE UNIQUE INDEX on_chain_txid_on_chain_vout ON utxos(on_chain_txid, on_chain_vout);
-- Recipient addresses only told change apart from payouts in the old view;
-- change is now identified by its output index, so nothing reads them anymore
DROP TABLE wallet_withdrawal_addresses;
-- The UTXO endpoint always filters by federation
CREATE INDEX utxos_federation_id ON utxos(federation_id);
INSERT INTO schema_version(version) VALUES (11);
