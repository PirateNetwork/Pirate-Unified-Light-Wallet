use pirate_wallet_service::WalletService;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[cfg(target_os = "android")]
use jni::objects::{JClass, JString};
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jstring};
#[cfg(target_os = "android")]
use jni::JNIEnv;

fn owned_json_ptr(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => {
            CString::new("{\"ok\":false,\"error\":\"response contains interior null bytes\"}")
                .expect("static JSON is valid")
                .into_raw()
        }
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `ptr` must be a pointer previously returned by this library via
/// [`pirate_wallet_service_invoke_json`] or another compatible allocator path.
/// It must not be freed more than once and must not be used after this call.
pub unsafe extern "C" fn pirate_wallet_service_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[unsafe(no_mangle)]
/// # Safety
///
/// `request_json` must be a valid, NUL-terminated UTF-8 C string pointer for
/// the duration of this call.
pub unsafe extern "C" fn pirate_wallet_service_invoke_json(
    request_json: *const c_char,
    pretty: bool,
) -> *mut c_char {
    if request_json.is_null() {
        return owned_json_ptr("{\"ok\":false,\"error\":\"request_json was null\"}".to_string());
    }

    let request = unsafe { CStr::from_ptr(request_json) };
    let request = match request.to_str() {
        Ok(value) => value,
        Err(_) => {
            return owned_json_ptr(
                "{\"ok\":false,\"error\":\"request_json was not valid UTF-8\"}".to_string(),
            )
        }
    };

    let service = WalletService::new();
    owned_json_ptr(service.execute_json(request, pretty))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pirate_wallet_sdk_NativeBridge_invokeJson(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JString,
    pretty: jboolean,
) -> jstring {
    let request_json = match env.get_string(&request_json) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(err) => {
            let fallback = format!(
                "{{\"ok\":false,\"error\":\"failed to read request string: {}\"}}",
                err
            );
            return env
                .new_string(fallback)
                .expect("static JSON must be valid")
                .into_raw();
        }
    };

    let service = WalletService::new();
    let response = service.execute_json(&request_json, pretty != 0);
    env.new_string(response)
        .expect("response JSON must be valid UTF-8")
        .into_raw()
}
