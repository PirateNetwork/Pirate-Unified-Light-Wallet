//! Change-address policy shared by transaction builders.

use pirate_params::{Network, NetworkType};

/// Returns true when new Sapling change outputs should use ZIP-32 internal scope.
///
/// Sapling internal change is enabled at the same network height as Ironwood/NU6.3.
/// Before that activation, Sapling-only transactions keep the legacy behavior of
/// returning change to the first selected Sapling spend address.
pub fn sapling_internal_change_active(network_type: NetworkType, target_height: u64) -> bool {
    let network = Network::from_type(network_type);
    match u32::try_from(target_height) {
        Ok(height) => network.is_ironwood_active(height),
        Err(_) => network.ironwood_activation_height.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirate_params::network::TESTNET_IRONWOOD_ACTIVATION_HEIGHT;

    #[test]
    fn mainnet_keeps_legacy_sapling_change_until_activation_is_configured() {
        assert!(!sapling_internal_change_active(
            NetworkType::Mainnet,
            u64::from(u32::MAX)
        ));
    }

    #[test]
    fn testnet_activates_sapling_internal_change_at_ironwood_height() {
        assert!(!sapling_internal_change_active(
            NetworkType::Testnet,
            u64::from(TESTNET_IRONWOOD_ACTIVATION_HEIGHT - 1)
        ));
        assert!(sapling_internal_change_active(
            NetworkType::Testnet,
            u64::from(TESTNET_IRONWOOD_ACTIVATION_HEIGHT)
        ));
    }

    #[test]
    fn regtest_activates_sapling_internal_change_at_ironwood_height() {
        assert!(!sapling_internal_change_active(NetworkType::Regtest, 199));
        assert!(sapling_internal_change_active(NetworkType::Regtest, 200));
    }
}
