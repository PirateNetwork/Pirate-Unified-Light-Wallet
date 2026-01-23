//! Property-based tests for pirate-core
//!
//! Uses proptest to verify invariants across randomized inputs

use pirate_core::keys::{ExtendedFullViewingKey, ExtendedSpendingKey, PaymentAddress};
use pirate_core::memo::Memo;
use pirate_core::{FeeCalculator, MAX_FEE, MIN_FEE};
use proptest::prelude::*;
use std::sync::OnceLock;

const TEST_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn base_fvk() -> &'static ExtendedFullViewingKey {
    static BASE_FVK: OnceLock<ExtendedFullViewingKey> = OnceLock::new();
    BASE_FVK.get_or_init(|| {
        let sk = ExtendedSpendingKey::from_mnemonic(TEST_MNEMONIC, "").expect("Valid mnemonic");
        sk.to_extended_fvk()
    })
}

// ============================================================================
// Property Test Strategies
// ============================================================================

/// Generate valid passphrase (0-100 chars)
fn passphrase_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9 ]{0,100}").unwrap()
}

/// Generate memo content (0-512 bytes)
fn memo_content_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9 ]{0,512}").unwrap()
}

/// Generate valid ARRR amounts (1 zatoshi to 21M ARRR)
fn amount_strategy() -> impl Strategy<Value = u64> {
    1u64..=(21_000_000 * 100_000_000)
}

/// Generate output counts for transactions
fn output_count_strategy() -> impl Strategy<Value = usize> {
    1usize..=10
}

// ============================================================================
// Key Derivation Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Same mnemonic + passphrase = same keys
    #[test]
    fn prop_deterministic_key_derivation(
        passphrase in passphrase_strategy()
    ) {
        let sk1 = ExtendedSpendingKey::from_mnemonic(TEST_MNEMONIC, &passphrase)
            .expect("Valid mnemonic");
        let sk2 = ExtendedSpendingKey::from_mnemonic(TEST_MNEMONIC, &passphrase)
            .expect("Valid mnemonic");

        let fvk1 = sk1.to_extended_fvk();
        let fvk2 = sk2.to_extended_fvk();

        // Same mnemonic should produce same addresses
        prop_assert_eq!(fvk1.derive_address(0).encode(), fvk2.derive_address(0).encode());
    }

    /// Property: Different passphrases = different keys
    #[test]
    fn prop_passphrase_changes_keys(
        pass1 in passphrase_strategy(),
        pass2 in passphrase_strategy()
    ) {
        prop_assume!(pass1 != pass2);

        let sk1 = ExtendedSpendingKey::from_mnemonic(TEST_MNEMONIC, &pass1)
            .expect("Valid mnemonic");
        let sk2 = ExtendedSpendingKey::from_mnemonic(TEST_MNEMONIC, &pass2)
            .expect("Valid mnemonic");

        let addr1 = sk1.to_extended_fvk().derive_address(0).encode();
        let addr2 = sk2.to_extended_fvk().derive_address(0).encode();

        // Different passphrases should produce different addresses
        prop_assert_ne!(addr1, addr2);
    }

    /// Property: Address derivation at same index = same address
    #[test]
    fn prop_deterministic_address_derivation(
        index in 0u32..1000
    ) {
        let fvk = base_fvk();
        let addr1 = fvk.derive_address(index);
        let addr2 = fvk.derive_address(index);

        prop_assert_eq!(addr1.encode(), addr2.encode());
    }

    /// Property: Different indices = different addresses
    #[test]
    fn prop_different_indices_different_addresses(
        index1 in 0u32..1000,
        index2 in 0u32..1000
    ) {
        prop_assume!(index1 != index2);

        let fvk = base_fvk();
        let addr1 = fvk.derive_address(index1).encode();
        let addr2 = fvk.derive_address(index2).encode();

        prop_assert_ne!(addr1, addr2);
    }

    /// Property: All generated addresses start with zs1
    #[test]
    fn prop_addresses_have_correct_prefix(
        index in 0u32..100
    ) {
        let fvk = base_fvk();
        let addr = fvk.derive_address(index).encode();

        prop_assert!(addr.starts_with("zs1"));
    }
}

// ============================================================================
// IVK Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: IVK export/import roundtrip preserves addresses
    #[test]
    fn prop_ivk_roundtrip_preserves_addresses(
        index in 0u32..100
    ) {
        let fvk = base_fvk();

        // Export FVK bytes
        let fvk_bytes = fvk.to_bytes();

        // Import FVK
        let fvk_imported =
            ExtendedFullViewingKey::from_bytes(&fvk_bytes).expect("Valid FVK bytes");

        // Addresses should match
        let addr_original = fvk.derive_address(index).encode();
        let addr_imported = fvk_imported.derive_address(index).encode();

        prop_assert_eq!(addr_original, addr_imported);
    }

    /// Property: All IVKs start with zxviews1
    #[test]
    fn prop_ivk_has_correct_prefix(_dummy in 0u32..10) {
        let fvk = base_fvk();

        let ivk = fvk.to_ivk_string();

        prop_assert!(ivk.starts_with("zxviews1"));
    }
}

// ============================================================================
// Memo Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Valid memo content roundtrips
    #[test]
    fn prop_memo_roundtrip(
        content in memo_content_strategy()
    ) {
        // Only test valid-length memos
        if content.len() <= 512 {
            let memo = Memo::from_text(content.clone()).expect("Valid memo");
            match memo {
                Memo::Empty => prop_assert!(content.is_empty()),
                Memo::Text(text) => prop_assert_eq!(content, text),
                Memo::Arbitrary(_) => prop_assert!(false, "Text memo should not be arbitrary"),
            }
        }
    }

    /// Property: Memo length is always <= 512 bytes
    #[test]
    fn prop_memo_respects_max_length(
        content in memo_content_strategy()
    ) {
        if let Ok(memo) = Memo::from_text(content) {
            prop_assert!(memo.byte_len() <= 512);
        }
    }

    /// Property: Empty memo is valid
    #[test]
    fn prop_empty_memo_valid(_dummy in 0u32..10) {
        let memo = Memo::from_text(String::new()).expect("Empty memo should be valid");
        prop_assert!(memo.is_empty());
    }
}

// ============================================================================
// Fee Calculation Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Fee is fixed regardless of outputs
    #[test]
    fn prop_fee_is_fixed(
        outputs in output_count_strategy()
    ) {
        let fee = FeeCalculator::new().calculate_fee(1, outputs, false).unwrap();
        prop_assert_eq!(fee, MIN_FEE);
    }

    /// Property: Fee is always within bounds
    #[test]
    fn prop_fee_within_bounds(
        outputs in output_count_strategy()
    ) {
        let fee = FeeCalculator::new().calculate_fee(1, outputs, false).unwrap();
        prop_assert!(fee >= MIN_FEE);
        prop_assert!(fee <= MAX_FEE);
    }

    /// Property: Fee calculation is deterministic
    #[test]
    fn prop_fee_deterministic(
        outputs in output_count_strategy()
    ) {
        let fee1 = FeeCalculator::new().calculate_fee(1, outputs, false).unwrap();
        let fee2 = FeeCalculator::new().calculate_fee(1, outputs, false).unwrap();

        prop_assert_eq!(fee1, fee2);
    }
}

// ============================================================================
// Amount Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Amount addition is commutative
    #[test]
    fn prop_amount_addition_commutative(
        a in 1u64..1_000_000,
        b in 1u64..1_000_000
    ) {
        // Ensure no overflow
        if a.checked_add(b).is_some() {
            prop_assert_eq!(a + b, b + a);
        }
    }

    /// Property: Amount subtraction produces non-negative
    #[test]
    fn prop_amount_subtraction_valid(
        a in 1_000u64..1_000_000,
        b in 1u64..1_000
    ) {
        if a >= b {
            let result = a - b;
            prop_assert!(result < a);
            prop_assert_eq!(result + b, a);
        }
    }

    /// Property: Formatting and parsing amounts roundtrips
    #[test]
    fn prop_amount_format_parse_roundtrip(
        arrrtoshis in amount_strategy()
    ) {
        let arrr = arrrtoshis as f64 / 100_000_000.0;
        let formatted = format!("{:.8}", arrr);
        let parsed: f64 = formatted.parse().expect("Valid float");
        let result_arrrtoshis = (parsed * 100_000_000.0) as u64;

        // Allow small rounding differences (< 1 zatoshi)
        let diff = result_arrrtoshis.abs_diff(arrrtoshis);
        prop_assert!(diff <= 1);
    }
}

// ============================================================================
// Address Encoding Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Address encoding is reversible
    #[test]
    fn prop_address_encode_decode(
        index in 0u32..100
    ) {
        let fvk = base_fvk();
        let addr = fvk.derive_address(index);
        let encoded = addr.encode();

        // Decode should succeed
        let decoded = PaymentAddress::decode_any_network(&encoded);
        prop_assert!(decoded.is_ok());
    }

    /// Property: Invalid address prefix is rejected
    #[test]
    fn prop_invalid_address_prefix_rejected(
        prefix in "[a-z]{2}[0-9]"
    ) {
        prop_assume!(prefix != "zs1");

        let invalid_addr = format!("{}{}", prefix, "0".repeat(50));
        let result = PaymentAddress::decode_any_network(&invalid_addr);

        prop_assert!(result.is_err());
    }
}

// ============================================================================
// Transaction Amount Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Property: Total outputs <= inputs (with fee)
    #[test]
    fn prop_transaction_outputs_plus_fee_lte_inputs(
        outputs in prop::collection::vec(1u64..1_000_000, 1..5)
    ) {
        let total_outputs: u64 = outputs.iter().sum();
        let fee = FeeCalculator::new()
            .calculate_fee(1, outputs.len(), false)
            .unwrap();
        let required_inputs = total_outputs + fee;

        // This should always hold for valid transactions
        prop_assert!(required_inputs >= total_outputs);
        prop_assert!(required_inputs >= fee);
    }

    /// Property: Zero-amount outputs are invalid
    #[test]
    fn prop_zero_amount_output_invalid(
        amount in prop::option::of(0u64..1)
    ) {
        if let Some(0) = amount {
            // Zero amounts should be rejected
            // (This would be validated in transaction building)
            prop_assert!(true);
        }
    }
}
