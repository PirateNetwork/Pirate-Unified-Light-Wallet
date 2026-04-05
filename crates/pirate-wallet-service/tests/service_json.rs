use pirate_wallet_service::WalletService;
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
    assert!(parsed["result"]["default_fee"].as_u64().is_some());
    assert!(parsed["result"]["min_fee"].as_u64().is_some());
}
