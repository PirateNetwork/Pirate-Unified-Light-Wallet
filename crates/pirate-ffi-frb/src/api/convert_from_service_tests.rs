//! Regression tests for the FRB typed-bridge conversion helpers in `crate::api`.
//!
//! `convert_from_service` re-serializes `pirate-wallet-service` values through
//! JSON so the FRB-facing model types can be deserialized from them. The service
//! serializes 64-bit amounts as strings (JS-safe), so these tests pin that the
//! conversion still accepts stringified amounts/cursors. This concern exists only
//! in this crate; the service has no equivalent, which is why these tests stayed
//! when the rest of the duplicated regression suite was removed.

use super::*;

#[test]
fn test_convert_from_service_accepts_stringified_activity_amounts() {
    let service_txs = vec![pirate_wallet_service::TxInfo {
        txid: "activity-tx".to_string(),
        height: Some(2_345_678),
        timestamp: 1_710_000_000,
        amount: -94_293_752,
        fee: 1_000,
        memo: Some("activity memo".to_string()),
        confirmed: true,
        expired: false,
        expiry_height: None,
    }];

    let service_json = serde_json::to_value(&service_txs).unwrap();
    assert_eq!(service_json[0]["amount"], "-94293752");
    assert_eq!(service_json[0]["fee"], "1000");

    let converted: Vec<TxInfo> = convert_from_service(service_txs).unwrap();
    assert_eq!(converted[0].amount, -94_293_752);
    assert_eq!(converted[0].fee, 1_000);
}

#[test]
fn test_convert_from_service_accepts_stringified_transaction_page_cursor() {
    let service_page = pirate_wallet_service::TransactionPage {
        transactions: vec![pirate_wallet_service::TxInfo {
            txid: "activity-tx".to_string(),
            height: Some(2_345_678),
            timestamp: 1_710_000_000,
            amount: -94_293_752,
            fee: 1_000,
            memo: None,
            confirmed: true,
            expired: false,
            expiry_height: None,
        }],
        next_cursor: Some(pirate_wallet_service::TransactionCursor {
            height: Some(2_345_678),
            txid: "activity-tx".to_string(),
            amount: -94_293_752,
        }),
    };

    let service_json = serde_json::to_value(&service_page).unwrap();
    assert_eq!(service_json["next_cursor"]["amount"], "-94293752");

    let converted: TransactionPage = convert_from_service(service_page).unwrap();
    assert_eq!(converted.transactions[0].amount, -94_293_752);
    assert_eq!(converted.next_cursor.unwrap().amount, -94_293_752);
}

#[test]
fn test_convert_from_service_accepts_large_stringified_pending_amounts() {
    const LARGE_AMOUNT: u64 = 9_007_199_254_740_993;
    let service_pending = pirate_wallet_service::PendingTx {
        id: "pending-1".to_string(),
        outputs: vec![pirate_wallet_service::Output {
            addr: "zs1receiver".to_string(),
            amount: LARGE_AMOUNT,
            memo: None,
        }],
        total_amount: LARGE_AMOUNT,
        fee: 1_000,
        change: 0,
        input_total: LARGE_AMOUNT + 1_000,
        num_inputs: 1,
        expiry_height: 1_234_567,
        created_at: 1_710_000_001,
    };

    let service_json = serde_json::to_value(&service_pending).unwrap();
    assert_eq!(
        service_json["outputs"][0]["amount"],
        LARGE_AMOUNT.to_string()
    );
    assert_eq!(service_json["total_amount"], LARGE_AMOUNT.to_string());
    assert_eq!(
        service_json["input_total"],
        (LARGE_AMOUNT + 1_000).to_string()
    );

    let converted: PendingTx = convert_from_service(service_pending).unwrap();
    assert_eq!(converted.outputs[0].amount, LARGE_AMOUNT);
    assert_eq!(converted.total_amount, LARGE_AMOUNT);
    assert_eq!(converted.input_total, LARGE_AMOUNT + 1_000);
}

#[test]
fn test_convert_from_service_accepts_stringified_balance_and_fee_amounts() {
    const LARGE_AMOUNT: u64 = 9_007_199_254_740_993;
    let service_balance = pirate_wallet_service::Balance {
        total: LARGE_AMOUNT,
        spendable: LARGE_AMOUNT - 100,
        pending: 100,
    };
    let service_fee = pirate_wallet_service::FeeInfo {
        default_fee: 10_000,
        min_fee: 1_000,
        max_fee: 1_000_000,
        fee_per_output: 0,
        memo_fee_multiplier: 1.0,
    };

    let balance_json = serde_json::to_value(&service_balance).unwrap();
    let fee_json = serde_json::to_value(&service_fee).unwrap();
    assert_eq!(balance_json["total"], LARGE_AMOUNT.to_string());
    assert_eq!(fee_json["default_fee"], "10000");

    let converted_balance: Balance = convert_from_service(service_balance).unwrap();
    let converted_fee: FeeInfo = convert_from_service(service_fee).unwrap();
    assert_eq!(converted_balance.total, LARGE_AMOUNT);
    assert_eq!(converted_balance.spendable, LARGE_AMOUNT - 100);
    assert_eq!(converted_fee.default_fee, 10_000);
    assert_eq!(converted_fee.max_fee, 1_000_000);
}
