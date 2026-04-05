use std::ffi::{CStr, CString};

use pirate_ffi_native::{pirate_wallet_service_free_string, pirate_wallet_service_invoke_json};

#[test]
fn native_json_abi_returns_build_info() {
    let request = CString::new(r#"{"method":"get_build_info"}"#).expect("request CString");
    let response_ptr = unsafe { pirate_wallet_service_invoke_json(request.as_ptr(), false) };
    assert!(!response_ptr.is_null());

    let response = unsafe { CStr::from_ptr(response_ptr) }
        .to_str()
        .expect("UTF-8 response")
        .to_string();

    unsafe { pirate_wallet_service_free_string(response_ptr) };

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("response JSON");
    assert_eq!(parsed["ok"], true);
    assert!(parsed["result"]["git_commit"].as_str().is_some());
}

#[test]
fn native_json_abi_rejects_null_request() {
    let response_ptr = unsafe { pirate_wallet_service_invoke_json(std::ptr::null(), false) };
    assert!(!response_ptr.is_null());

    let response = unsafe { CStr::from_ptr(response_ptr) }
        .to_str()
        .expect("UTF-8 response")
        .to_string();

    unsafe { pirate_wallet_service_free_string(response_ptr) };

    let parsed: serde_json::Value = serde_json::from_str(&response).expect("response JSON");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["error"], "request_json was null");
}
