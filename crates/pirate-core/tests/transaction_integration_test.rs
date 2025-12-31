//! Integration tests for transaction building flow
//!
//! Tests the complete flow from note selection through transaction building

use pirate_core::{
    transaction::TransactionBuilder,
    keys::{ExtendedSpendingKey, PaymentAddress},
    selection::SelectableNote,
    Memo,
};

// Helper to create test notes
fn test_note(value: u64, height: u64, output_index: u32) -> SelectableNote {
    SelectableNote::new(
        value,
        vec![0u8; 32],  // commitment
        height,
        vec![0u8; 32],  // txid
        output_index,
    )
}

#[test]
fn test_transaction_builder_flow() {
    // Create address
    let addr = PaymentAddress::test_address();

    // Create builder and add output
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 100_000, None).unwrap();

    // Verify builder state
    assert_eq!(builder.output_count(), 1);
}

#[test]
fn test_build_pending_transaction() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 100_000, None).unwrap();

    // Create mock notes (with enough for fee)
    let notes = vec![
        test_note(250_000, 1000, 0),
    ];

    // Build pending transaction
    let pending = builder.build_pending(&notes).unwrap();
    
    assert_eq!(pending.output_value, 100_000);
    assert!(pending.fee > 0, "Fee should be calculated");
    assert!(pending.change > 0, "Should have change");
    assert_eq!(pending.input_value, 250_000);
}

#[test]
fn test_multiple_outputs() {
    let addr1 = PaymentAddress::test_address();
    let addr2 = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr1, 50_000, None).unwrap();
    builder.add_output(addr2, 75_000, Some(Memo::from("Test"))).unwrap();

    // Create mock notes with enough value
    let notes = vec![
        test_note(350_000, 1000, 0),
    ];

    // Build pending transaction
    let pending = builder.build_pending(&notes).unwrap();
    
    assert_eq!(pending.output_value, 125_000);
    assert!(pending.fee > 0);
    assert!(pending.change > 0);
}

#[test]
fn test_insufficient_funds() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 100_000, None).unwrap();

    // Create mock notes with insufficient value
    let notes = vec![
        test_note(50_000, 1000, 0),
    ];

    // Should fail with insufficient funds
    let result = builder.build_pending(&notes);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Insufficient"));
}

#[test]
fn test_zero_amount_rejected() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    let result = builder.add_output(addr, 0, None);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("zero"));
}

#[test]
fn test_memo_validation() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    
    // Valid memo
    let valid_memo = Memo::from("This is a valid memo");
    let result = builder.add_output(addr.clone(), 100_000, Some(valid_memo));
    assert!(result.is_ok());
    
    // Too long memo
    let long_text = "a".repeat(600);
    let long_memo = Memo::from_text(long_text);
    let result2 = long_memo.is_err();
    assert!(result2);
}

#[test]
fn test_note_selection_with_multiple_notes() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 100_000, None).unwrap();

    // Create multiple mock notes
    let notes = vec![
        test_note(80_000, 1000, 0),
        test_note(100_000, 1001, 0),
        test_note(120_000, 1002, 0),
    ];

    // Build pending transaction
    let pending = builder.build_pending(&notes).unwrap();
    
    assert_eq!(pending.output_value, 100_000);
    // Should select sufficient notes
    assert!(pending.input_value >= 100_000 + pending.fee);
}

#[test]
fn test_change_calculation() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 50_000, None).unwrap();

    let notes = vec![
        test_note(200_000, 1000, 0),
    ];

    let pending = builder.build_pending(&notes).unwrap();
    
    // Change should be: input - output - fee
    let expected_change = pending.input_value - pending.output_value - pending.fee;
    assert_eq!(pending.change, expected_change);
}

#[test]
fn test_fee_same_with_memo() {
    let addr = PaymentAddress::test_address();
    
    // Without memo
    let mut builder1 = TransactionBuilder::new();
    builder1.add_output(addr.clone(), 100_000, None).unwrap();
    
    let notes1 = vec![test_note(250_000, 1000, 0)];
    let pending1 = builder1.build_pending(&notes1).unwrap();
    
    // With memo
    let mut builder2 = TransactionBuilder::new();
    builder2.add_output(addr, 100_000, Some(Memo::from("Test memo"))).unwrap();
    
    let notes2 = vec![test_note(250_000, 1000, 0)];
    let pending2 = builder2.build_pending(&notes2).unwrap();
    
    // Fee should be identical for fixed-fee policy
    assert_eq!(
        pending2.fee,
        pending1.fee,
        "Fee with memo ({}) should match without ({})",
        pending2.fee,
        pending1.fee
    );
}

#[test]
fn test_send_to_many() {
    let mut builder = TransactionBuilder::new();
    
    // Add 5 outputs
    for i in 0..5 {
        let addr = PaymentAddress::test_address();
        let amount = 10_000 * (i + 1);
        builder.add_output(addr, amount, None).unwrap();
    }
    
    // Total output: 10k + 20k + 30k + 40k + 50k = 150k
    let notes = vec![
        test_note(500_000, 1000, 0),
    ];
    
    let pending = builder.build_pending(&notes).unwrap();
    assert_eq!(pending.output_value, 150_000);
    assert_eq!(pending.outputs.len(), 5);
}

#[test]
fn test_mnemonic_generation_and_restoration() {
    // Generate new mnemonic
    let mnemonic = ExtendedSpendingKey::generate_mnemonic();
    
    // Should be 24 words
    let words: Vec<&str> = mnemonic.split_whitespace().collect();
    assert_eq!(words.len(), 24);
    
    // Should be restorable
    let result = ExtendedSpendingKey::from_mnemonic(&mnemonic, "");
    assert!(result.is_ok());
}

#[test]
fn test_address_derivation() {
    let mnemonic = ExtendedSpendingKey::generate_mnemonic();
    let xsk = ExtendedSpendingKey::from_mnemonic(&mnemonic, "").unwrap();
    
    // Derive extended full viewing key
    let xfvk = xsk.to_extended_fvk();
    
    // Derive addresses
    let addr1 = xfvk.derive_address(0);
    let addr2 = xfvk.derive_address(1);
    
    // Addresses should be different
    assert_ne!(addr1, addr2);
}

#[test]
fn test_custom_fee() {
    let addr = PaymentAddress::test_address();
    
    let mut builder = TransactionBuilder::new();
    builder.add_output(addr, 100_000, None).unwrap();
    builder.with_fee_per_action(20_000); // Double the default fee
    
    let notes = vec![test_note(250_000, 1000, 0)];
    let pending = builder.build_pending(&notes).unwrap();
    
    // Fee should match custom rate
    assert_eq!(pending.fee, 20_000);
}

