use pirate_wallet_service::{TransactionCursor, WalletService, WalletServiceRequest};
use serde_json::Value;

#[test]
fn execute_json_returns_error_for_invalid_json() {
    let service = WalletService::new();
    let response = service.execute_json("{", false);
    let parsed: Value = serde_json::from_str(&response).expect("response is valid JSON");

    assert_eq!(parsed["ok"], Value::Bool(false));
    assert!(parsed["error"]
        .as_str()
        .expect("error string")
        .contains("Invalid request JSON"));
}

#[test]
fn execute_json_supports_build_info_request() {
    let service = WalletService::new();
    let response = service.execute_json(r#"{"method":"get_build_info"}"#, false);
    let parsed: Value = serde_json::from_str(&response).expect("response is valid JSON");

    assert_eq!(parsed["ok"], Value::Bool(true));
    assert!(parsed["result"]["version"].as_str().is_some());
    assert!(parsed["result"]["git_commit"].as_str().is_some());
}

#[test]
fn execute_json_supports_fee_info_request() {
    let service = WalletService::new();
    let response = service.execute_json(r#"{"method":"get_fee_info"}"#, false);
    let parsed: Value = serde_json::from_str(&response).expect("response is valid JSON");

    assert_eq!(parsed["ok"], Value::Bool(true));
    assert!(parsed["result"]["default_fee"].as_str().is_some());
    assert!(parsed["result"]["min_fee"].as_str().is_some());
}

#[test]
fn execute_json_serializes_parse_amount_result_as_string() {
    let service = WalletService::new();
    let response = service.execute_json(r#"{"method":"parse_amount","arrr":"1.25"}"#, false);
    let parsed: Value = serde_json::from_str(&response).expect("response is valid JSON");

    assert_eq!(parsed["ok"], Value::Bool(true));
    assert_eq!(parsed["result"], Value::String("125000000".to_string()));
}

#[test]
fn execute_json_accepts_string_amount_request_fields() {
    let service = WalletService::new();
    let response = service.execute_json(
        r#"{"method":"format_amount","arrrtoshis":"125000000"}"#,
        false,
    );
    let parsed: Value = serde_json::from_str(&response).expect("response is valid JSON");

    assert_eq!(parsed["ok"], Value::Bool(true));
    assert_eq!(parsed["result"], Value::String("1.25000000".to_string()));
}

#[test]
fn transaction_page_request_accepts_a_string_cursor_amount() {
    let request: WalletServiceRequest = serde_json::from_str(
        r#"{"method":"list_transactions_page","wallet_id":"wallet-1","cursor":{"height":42,"txid":"tx-1","amount":"-9007199254740993"},"page_size":50}"#,
    )
    .expect("transaction page request is valid");

    match request {
        WalletServiceRequest::ListTransactionsPage {
            wallet_id,
            cursor:
                Some(TransactionCursor {
                    height,
                    txid,
                    amount,
                }),
            page_size,
        } => {
            assert_eq!(wallet_id, "wallet-1");
            assert_eq!(height, Some(42));
            assert_eq!(txid, "tx-1");
            assert_eq!(amount, -9_007_199_254_740_993);
            assert_eq!(page_size, 50);
        }
        other => panic!("unexpected request: {other:?}"),
    }
}
