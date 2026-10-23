use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Decodable;
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fedimint_core::TransactionId;
use postgres_from_row::FromRow;
use tokio_postgres::{Error, Row};

#[derive(Debug, Clone)]
pub struct Federation {
    pub federation_id: FederationId,
    pub config: ClientConfig,
}

impl FromRow for Federation {
    fn from_row(row: &Row) -> Self {
        Self::try_from_row(row).expect("Decoding row failed")
    }

    fn try_from_row(row: &Row) -> Result<Self, Error> {
        let federation_id_bytes: Vec<u8> = row.try_get("federation_id")?;
        let federation_id =
            FederationId::consensus_decode_whole(&federation_id_bytes, &Default::default())
                .expect("Invalid data in DB");

        let config_bytes: Vec<u8> = row.try_get("config")?;
        let config = ClientConfig::consensus_decode_whole(&config_bytes, &Default::default())
            .expect("Invalid data in DB");

        Ok(Federation {
            federation_id,
            config,
        })
    }
}

pub struct Transaction {
    pub txid: TransactionId,
    #[allow(dead_code)]
    pub session_index: i32,
    #[allow(dead_code)]
    pub item_index: i32,
    pub data: fedimint_core::transaction::Transaction,
}

impl FromRow for crate::federation::db::Transaction {
    fn from_row(row: &Row) -> Self {
        Self::try_from_row(row).expect("Decoding row failed")
    }

    fn try_from_row(row: &Row) -> Result<Self, Error> {
        let decoder = ModuleDecoderRegistry::default().with_fallback();

        let txid_bytes: Vec<u8> = row.try_get("txid")?;
        let txid = TransactionId::consensus_decode_whole(&txid_bytes, &decoder)
            .expect("Invalid data in DB");

        let session_index = row.try_get::<_, i32>("session_index")?;

        let item_index = row.try_get::<_, i32>("item_index")?;

        let data_bytes: Vec<u8> = row.try_get("data")?;
        let data =
            fedimint_core::transaction::Transaction::consensus_decode_whole(&data_bytes, &decoder)
                .expect("Invalid data in DB");

        Ok(crate::federation::db::Transaction {
            txid,
            session_index,
            item_index,
            data,
        })
    }
}

#[derive(Debug)]
pub struct SessionOutcome {
    pub session_index: i32,
    pub data: fedimint_core::session_outcome::SessionOutcome,
}

impl FromRow for crate::federation::db::SessionOutcome {
    fn from_row(row: &Row) -> Self {
        Self::try_from_row(row).expect("Decoding row failed")
    }

    fn try_from_row(row: &Row) -> Result<Self, Error> {
        let decoder = ModuleDecoderRegistry::default().with_fallback();

        let session_data_bytes: Vec<u8> = row.try_get("session")?;
        let data = fedimint_core::session_outcome::SessionOutcome::consensus_decode_whole(
            &session_data_bytes,
            &decoder,
        )
        .expect("Invalid data in DB");

        let session_index = row.try_get::<_, i32>("session_index")?;

        Ok(crate::federation::db::SessionOutcome {
            session_index,
            data,
        })
    }
}

impl SessionOutcome {
    pub fn from_row_with_decoders(row: &Row, decoders: &ModuleDecoderRegistry) -> Self {
        Self::try_from_row_with_decoders(row, decoders).expect("Decoding row failed")
    }

    pub fn try_from_row_with_decoders(
        row: &Row,
        decoders: &ModuleDecoderRegistry,
    ) -> Result<Self, Error> {
        let session_data_bytes: Vec<u8> = row.try_get("session")?;
        let data = fedimint_core::session_outcome::SessionOutcome::consensus_decode_whole(
            &session_data_bytes,
            decoders,
        )
        .expect("Invalid data in DB");

        let session_index = row.try_get::<_, i32>("session_index")?;

        Ok(crate::federation::db::SessionOutcome {
            session_index,
            data,
        })
    }
}
