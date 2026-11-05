use fmo_core::FedimintObserver;
use fmo_module_ln::LnObserver;
use fmo_module_mint::MintObserver;
use fmo_module_wallet::WalletObserver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let builder = FedimintObserver::builder()
        .with_module(MintObserver)
        .with_module(WalletObserver::default())
        .with_module(LnObserver::default())
        .with_module(fmo_module_mintv2::MintV2Observer)
        .with_module(fmo_module_walletv2::WalletV2Observer)
        .with_module(fmo_module_lnv2::LnV2Observer);
    #[cfg(feature = "stability_pool_v1")]
    let builder = builder.with_module(fmo_module_stability_pool::StabilityPoolObserver);
    builder.run().await
}
