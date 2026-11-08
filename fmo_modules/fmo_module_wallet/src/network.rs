//! The Bitcoin network a federation's wallet runs on, for rendering addresses.
//! Addresses rendered for the wrong network (`bc1…` on a signet federation)
//! point at the right script but are useless to anyone copying them.

use std::str::FromStr;

use anyhow::Context;
use bitcoin::address::NetworkUnchecked;
use bitcoin::{Address, Network, Script};
use fedimint_core::config::{ClientConfig, ClientModuleConfig};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::{Decodable, DynRawFallback};
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fedimint_core::module::CommonModuleInit;
use fedimint_wallet_common::config::WalletClientConfig;
use fedimint_wallet_common::WalletCommonInit;

/// The network in a wallet module's client config. Stored configs may not have
/// been decoded with the module's decoder, so this decodes raw ones itself
/// rather than relying on `ClientModuleConfig::cast`, which panics on them.
fn wallet_network(module: &ClientModuleConfig) -> anyhow::Result<Network> {
    anyhow::ensure!(
        module.kind == WalletCommonInit::KIND,
        "Module is a {} module, not a wallet module",
        module.kind
    );
    let network = match &module.config {
        DynRawFallback::Raw { raw, .. } => {
            WalletClientConfig::consensus_decode_whole(raw, &ModuleDecoderRegistry::default())
                .context("Wallet client config does not decode")?
                .network
        }
        DynRawFallback::Decoded(config) => config
            .as_any()
            .downcast_ref::<WalletClientConfig>()
            .context("Wallet client config has an unexpected type")?
            .network
            .clone(),
    };
    Ok(network.0)
}

/// The network of the federation's wallet module `instance`
pub(crate) fn federation_wallet_network(
    config: &ClientConfig,
    instance: ModuleInstanceId,
) -> anyhow::Result<Network> {
    wallet_network(
        config
            .modules
            .get(&instance)
            .with_context(|| format!("Federation config has no module instance {instance}"))?,
    )
}

/// The address paying to `script` on `network`
pub(crate) fn script_address(script: &Script, network: Network) -> anyhow::Result<Address> {
    Address::from_script(script, network).context("Output script has no address form")
}

/// A stored address rendered for `network`. Rows written before addresses
/// followed the federation's network hold mainnet renderings; the script they
/// encode is the same on every network, so this serves them correctly without
/// rewriting them.
pub(crate) fn address_on_network(
    stored: &str,
    network: Network,
) -> anyhow::Result<Address<NetworkUnchecked>> {
    let script = Address::from_str(stored)?.assume_checked().script_pubkey();
    Ok(script_address(&script, network)?.into_unchecked())
}

#[cfg(test)]
mod tests {
    use fedimint_core::encoding::btc::NetworkLegacyEncodingWrapper;
    use fedimint_core::encoding::Encodable;
    use fedimint_core::envs::BitcoinRpcConfig;
    use fedimint_core::module::ModuleConsensusVersion;
    use fedimint_core::util::SafeUrl;
    use fedimint_wallet_common::config::FeeConsensus;
    use fedimint_wallet_common::keys::CompressedPublicKey;
    use fedimint_wallet_common::PegInDescriptor;

    use super::*;

    fn wallet_config(network: Network) -> WalletClientConfig {
        let key = CompressedPublicKey::new(
            bitcoin::secp256k1::PublicKey::from_str(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )
            .unwrap(),
        );
        WalletClientConfig {
            peg_in_descriptor: PegInDescriptor::new_wpkh(key).unwrap(),
            network: NetworkLegacyEncodingWrapper(network),
            finality_delay: 6,
            fee_consensus: FeeConsensus::default(),
            default_bitcoin_rpc: BitcoinRpcConfig {
                kind: "esplora".to_owned(),
                url: SafeUrl::parse("https://mutinynet.com/api/").unwrap(),
            },
        }
    }

    #[test]
    fn network_comes_from_decoded_and_raw_configs() {
        for network in [
            Network::Bitcoin,
            Network::Signet,
            Network::Testnet,
            Network::Regtest,
        ] {
            let decoded = ClientModuleConfig::from_typed(
                0,
                WalletCommonInit::KIND,
                ModuleConsensusVersion::new(2, 0),
                wallet_config(network),
            )
            .unwrap();
            assert_eq!(wallet_network(&decoded).unwrap(), network);

            let raw = ClientModuleConfig {
                config: DynRawFallback::Raw {
                    module_instance_id: 0,
                    raw: wallet_config(network).consensus_encode_to_vec(),
                },
                ..decoded
            };
            assert_eq!(wallet_network(&raw).unwrap(), network);
        }
    }

    #[test]
    fn other_module_kinds_have_no_wallet_network() {
        let module = ClientModuleConfig {
            kind: fedimint_core::core::ModuleKind::from_static_str("mint"),
            version: ModuleConsensusVersion::new(2, 0),
            config: DynRawFallback::Raw {
                module_instance_id: 1,
                raw: vec![],
            },
        };
        assert!(wallet_network(&module).is_err());
    }

    #[test]
    fn addresses_follow_the_federation_network() {
        // A mainnet rendering, as rows written before this fix hold
        let stored = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
        let script = Address::from_str(stored)
            .unwrap()
            .assume_checked()
            .script_pubkey();
        assert_eq!(
            address_on_network(stored, Network::Bitcoin)
                .unwrap()
                .assume_checked()
                .to_string(),
            stored
        );
        for (network, prefix) in [
            (Network::Signet, "tb1q"),
            (Network::Testnet, "tb1q"),
            (Network::Regtest, "bcrt1q"),
        ] {
            let address = address_on_network(stored, network)
                .unwrap()
                .require_network(network)
                .unwrap();
            assert!(address.to_string().starts_with(prefix), "{address}");
            assert_eq!(address.script_pubkey(), script);
            assert_eq!(
                script_address(&script, network).unwrap().to_string(),
                address.to_string()
            );
        }
    }
}
