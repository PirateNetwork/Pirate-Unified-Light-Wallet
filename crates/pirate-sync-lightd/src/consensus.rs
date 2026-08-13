//! Consensus compatibility checks for lightwalletd connections.

use crate::{Error, Result};
use pirate_core::transaction::PirateNetwork;
use pirate_params::NetworkType;
use zcash_protocol::consensus::{BlockHeight, BranchId};

/// Exact consensus-branch comparison at a server-reported chain height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusBranchCheck {
    /// Height at which both branch identifiers were evaluated.
    pub height: u32,
    /// Branch identifier selected by the SDK's network schedule.
    pub sdk_branch_id: String,
    /// Normalized branch identifier reported by the server, when parseable.
    pub server_branch_id: Option<String>,
}

impl ConsensusBranchCheck {
    /// Returns whether the SDK and server report the same opaque branch identifier.
    pub fn is_valid(&self) -> bool {
        self.server_branch_id.as_deref() == Some(self.sdk_branch_id.as_str())
    }

    /// Rejects a server that does not exactly match the SDK's consensus branch.
    pub fn require_match(&self) -> Result<()> {
        if self.is_valid() {
            return Ok(());
        }

        Err(Error::Network(format!(
            "Incompatible consensus branch at height {}: SDK expects {}, server reports {}. Update the wallet or switch to a compatible server.",
            self.height,
            self.sdk_branch_id,
            self.server_branch_id.as_deref().unwrap_or("unrecognized")
        )))
    }
}

/// Compares lightwalletd's branch identifier with the SDK schedule at the same height.
pub fn check_consensus_branch(
    network_type: NetworkType,
    server_height: u64,
    server_branch_id: &str,
) -> Result<ConsensusBranchCheck> {
    check_consensus_branch_with_activation_height(
        network_type,
        server_height,
        server_branch_id,
        None,
    )
}

/// Compares a server branch with a schedule containing a resolved Ironwood height.
pub fn check_consensus_branch_with_activation_height(
    network_type: NetworkType,
    server_height: u64,
    server_branch_id: &str,
    ironwood_activation_height: Option<u32>,
) -> Result<ConsensusBranchCheck> {
    let height = u32::try_from(server_height)
        .map_err(|_| Error::Network(format!("Server height out of range: {server_height}")))?;
    let network =
        PirateNetwork::with_ironwood_activation_height(network_type, ironwood_activation_height);
    let sdk_branch = BranchId::for_height(&network, BlockHeight::from_u32(height));

    Ok(ConsensusBranchCheck {
        height,
        sdk_branch_id: format_branch_id(u32::from(sdk_branch)),
        server_branch_id: parse_branch_id(server_branch_id).map(format_branch_id),
    })
}

fn parse_branch_id(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    let normalized = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u32::from_str_radix(normalized, 16).ok()
}

fn format_branch_id(value: u32) -> String {
    format!("{value:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirate_params::network::TESTNET_IRONWOOD_ACTIVATION_HEIGHT;

    fn sdk_branch_at(network_type: NetworkType, height: u32) -> String {
        let network = PirateNetwork::new(network_type);
        format_branch_id(u32::from(BranchId::for_height(
            &network,
            BlockHeight::from_u32(height),
        )))
    }

    #[test]
    fn accepts_an_exact_branch_match() {
        let expected = sdk_branch_at(NetworkType::Mainnet, 4_000_000);
        let check = check_consensus_branch(
            NetworkType::Mainnet,
            4_000_000,
            &format!("0X{}", expected.to_uppercase()),
        )
        .unwrap();

        assert_eq!(check.server_branch_id.as_deref(), Some(expected.as_str()));
        assert!(check.is_valid());
        assert!(check.require_match().is_ok());
    }

    #[test]
    fn rejects_a_different_opaque_branch_id() {
        let check = check_consensus_branch(NetworkType::Mainnet, 4_000_000, "ffffffff").unwrap();

        assert!(!check.is_valid());
        assert!(check
            .require_match()
            .unwrap_err()
            .to_string()
            .contains("Incompatible consensus branch"));
    }

    #[test]
    fn rejects_an_unparseable_server_branch_id() {
        let check =
            check_consensus_branch(NetworkType::Mainnet, 4_000_000, "not-a-branch").unwrap();

        assert_eq!(check.server_branch_id, None);
        assert!(!check.is_valid());
        assert!(check.require_match().is_err());
    }

    #[test]
    fn follows_the_testnet_ironwood_activation_boundary() {
        let before_height = TESTNET_IRONWOOD_ACTIVATION_HEIGHT - 1;
        let before_branch = sdk_branch_at(NetworkType::Testnet, before_height);
        let activation_branch =
            sdk_branch_at(NetworkType::Testnet, TESTNET_IRONWOOD_ACTIVATION_HEIGHT);

        assert_ne!(before_branch, activation_branch);
        assert!(
            check_consensus_branch(NetworkType::Testnet, before_height as u64, &before_branch,)
                .unwrap()
                .is_valid()
        );
        assert!(check_consensus_branch(
            NetworkType::Testnet,
            TESTNET_IRONWOOD_ACTIVATION_HEIGHT as u64,
            &activation_branch,
        )
        .unwrap()
        .is_valid());
    }

    #[test]
    fn follows_a_resolved_mainnet_activation_boundary() {
        let activation_height = 4_200_060;
        let network = PirateNetwork::with_ironwood_activation_height(
            NetworkType::Mainnet,
            Some(activation_height),
        );
        let ironwood_branch = format_branch_id(u32::from(BranchId::for_height(
            &network,
            BlockHeight::from_u32(activation_height),
        )));

        assert!(check_consensus_branch_with_activation_height(
            NetworkType::Mainnet,
            u64::from(activation_height),
            &ironwood_branch,
            Some(activation_height),
        )
        .unwrap()
        .is_valid());
    }
}
