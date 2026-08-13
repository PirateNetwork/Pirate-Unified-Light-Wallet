//! Change-address policy shared by transaction builders.

use pirate_params::{Network, NetworkType};

/// Returns true when new Sapling change outputs should use ZIP-32 internal scope.
///
/// Sapling internal change is enabled at the same network height as Ironwood/NU6.3.
/// Before that activation, Sapling-only transactions keep the legacy behavior of
/// returning change to the first selected Sapling spend address.
pub fn sapling_internal_change_active(network_type: NetworkType, target_height: u64) -> bool {
    sapling_internal_change_active_with_resolved_height(network_type, target_height, None)
}

/// Returns true when ZIP-32 internal change is active under a resolved schedule.
pub fn sapling_internal_change_active_with_resolved_height(
    network_type: NetworkType,
    target_height: u64,
    resolved_ironwood_activation_height: Option<u32>,
) -> bool {
    let network = Network::from_type(network_type);
    match u32::try_from(target_height) {
        Ok(height) => network
            .is_ironwood_active_with_resolved_height(height, resolved_ironwood_activation_height),
        Err(_) => network
            .ironwood_activation_height
            .or(resolved_ironwood_activation_height)
            .is_some(),
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
    fn mainnet_uses_internal_change_at_the_resolved_activation_height() {
        let activation_height = 4_200_060;

        assert!(!sapling_internal_change_active_with_resolved_height(
            NetworkType::Mainnet,
            u64::from(activation_height - 1),
            Some(activation_height)
        ));
        assert!(sapling_internal_change_active_with_resolved_height(
            NetworkType::Mainnet,
            u64::from(activation_height),
            Some(activation_height)
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
