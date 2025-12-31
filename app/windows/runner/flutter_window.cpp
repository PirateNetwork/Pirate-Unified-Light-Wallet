#include "flutter_window.h"

#include <filesystem>
#include <fstream>
#include <optional>
#include <string>
#include <vector>

#include <windows.h>
#include <dpapi.h>
#include <wincrypt.h>

#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include "flutter/generated_plugin_registrant.h"

namespace {
constexpr char kKeystoreChannelName[] = "com.pirate.wallet/keystore";
constexpr wchar_t kDpapiDescription[] = L"Pirate Wallet Key";

std::filesystem::path GetKeystoreDir() {
  DWORD length = GetEnvironmentVariableW(L"APPDATA", nullptr, 0);
  if (length == 0) {
    return std::filesystem::temp_directory_path() / L"PirateWallet" / L"keystore";
  }
  std::wstring buffer(length, L'\0');
  GetEnvironmentVariableW(L"APPDATA", buffer.data(), length);
  if (!buffer.empty() && buffer.back() == L'\0') {
    buffer.pop_back();
  }
  return std::filesystem::path(buffer) / L"PirateWallet" / L"keystore";
}

std::string HexEncode(const std::string& input) {
  static const char kHex[] = "0123456789abcdef";
  std::string out;
  out.reserve(input.size() * 2);
  for (unsigned char c : input) {
    out.push_back(kHex[c >> 4]);
    out.push_back(kHex[c & 0x0F]);
  }
  return out;
}

std::filesystem::path KeyPathForId(const std::string& key_id) {
  std::string hex = HexEncode(key_id);
  std::wstring filename = L"key_";
  filename.append(hex.begin(), hex.end());
  filename.append(L".bin");
  return GetKeystoreDir() / filename;
}

bool ProtectData(const std::vector<uint8_t>& input,
                 std::vector<uint8_t>* output,
                 std::string* error) {
  if (input.empty()) {
    if (error) {
      *error = "Input data is empty";
    }
    return false;
  }
  DATA_BLOB in_blob;
  in_blob.pbData = const_cast<BYTE*>(input.data());
  in_blob.cbData = static_cast<DWORD>(input.size());
  DATA_BLOB out_blob;
  if (!CryptProtectData(&in_blob, kDpapiDescription, nullptr, nullptr, nullptr,
                        CRYPTPROTECT_UI_FORBIDDEN, &out_blob)) {
    if (error) {
      *error = "CryptProtectData failed";
    }
    return false;
  }
  output->assign(out_blob.pbData, out_blob.pbData + out_blob.cbData);
  LocalFree(out_blob.pbData);
  return true;
}

bool UnprotectData(const std::vector<uint8_t>& input,
                   std::vector<uint8_t>* output,
                   std::string* error) {
  if (input.empty()) {
    if (error) {
      *error = "Input data is empty";
    }
    return false;
  }
  DATA_BLOB in_blob;
  in_blob.pbData = const_cast<BYTE*>(input.data());
  in_blob.cbData = static_cast<DWORD>(input.size());
  DATA_BLOB out_blob;
  if (!CryptUnprotectData(&in_blob, nullptr, nullptr, nullptr, nullptr,
                          CRYPTPROTECT_UI_FORBIDDEN, &out_blob)) {
    if (error) {
      *error = "CryptUnprotectData failed";
    }
    return false;
  }
  output->assign(out_blob.pbData, out_blob.pbData + out_blob.cbData);
  LocalFree(out_blob.pbData);
  return true;
}

bool WriteFileBytes(const std::filesystem::path& path,
                    const std::vector<uint8_t>& data,
                    std::string* error) {
  std::error_code ec;
  std::filesystem::create_directories(path.parent_path(), ec);
  if (ec) {
    if (error) {
      *error = "Failed to create keystore directory";
    }
    return false;
  }
  std::ofstream out(path, std::ios::binary | std::ios::trunc);
  if (!out) {
    if (error) {
      *error = "Failed to open keystore file";
    }
    return false;
  }
  out.write(reinterpret_cast<const char*>(data.data()),
            static_cast<std::streamsize>(data.size()));
  if (!out) {
    if (error) {
      *error = "Failed to write keystore file";
    }
    return false;
  }
  return true;
}

bool ReadFileBytes(const std::filesystem::path& path,
                   std::vector<uint8_t>* data,
                   std::string* error) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    return false;
  }
  std::vector<uint8_t> buffer((std::istreambuf_iterator<char>(in)),
                              std::istreambuf_iterator<char>());
  if (!in && !in.eof()) {
    if (error) {
      *error = "Failed to read keystore file";
    }
    return false;
  }
  *data = std::move(buffer);
  return true;
}
}  // namespace

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  keystore_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kKeystoreChannelName,
          &flutter::StandardMethodCodec::GetInstance());

  keystore_channel_->SetMethodCallHandler(
      [this](const auto& call, auto result) {
        const auto& method = call.method_name();

        if (method == "getCapabilities") {
          flutter::EncodableMap caps;
          caps[flutter::EncodableValue("hasSecureHardware")] =
              flutter::EncodableValue(false);
          caps[flutter::EncodableValue("hasStrongBox")] =
              flutter::EncodableValue(false);
          caps[flutter::EncodableValue("hasSecureEnclave")] =
              flutter::EncodableValue(false);
          caps[flutter::EncodableValue("hasBiometrics")] =
              flutter::EncodableValue(false);
          result->Success(flutter::EncodableValue(caps));
          return;
        }

        const auto* args =
            std::get_if<flutter::EncodableMap>(call.arguments());
        if (!args) {
          result->Error("INVALID_ARGUMENT", "Arguments missing");
          return;
        }

        auto get_string = [args](const std::string& key,
                                 std::string* value) -> bool {
          auto it = args->find(flutter::EncodableValue(key));
          if (it == args->end() ||
              !std::holds_alternative<std::string>(it->second)) {
            return false;
          }
          *value = std::get<std::string>(it->second);
          return true;
        };

        auto get_bytes = [args](const std::string& key,
                                std::vector<uint8_t>* value) -> bool {
          auto it = args->find(flutter::EncodableValue(key));
          if (it == args->end() ||
              !std::holds_alternative<std::vector<uint8_t>>(it->second)) {
            return false;
          }
          *value = std::get<std::vector<uint8_t>>(it->second);
          return true;
        };

        if (method == "storeKey") {
          std::string key_id;
          std::vector<uint8_t> encrypted_key;
          if (!get_string("keyId", &key_id) ||
              !get_bytes("encryptedKey", &encrypted_key)) {
            result->Error("INVALID_ARGUMENT", "keyId and encryptedKey required");
            return;
          }
          std::vector<uint8_t> protected_data;
          std::string error;
          if (!ProtectData(encrypted_key, &protected_data, &error)) {
            result->Error("KEYSTORE_ERROR", error);
            return;
          }
          if (!WriteFileBytes(KeyPathForId(key_id), protected_data, &error)) {
            result->Error("KEYSTORE_ERROR", error);
            return;
          }
          result->Success(flutter::EncodableValue(true));
          return;
        }

        if (method == "retrieveKey") {
          std::string key_id;
          if (!get_string("keyId", &key_id)) {
            result->Error("INVALID_ARGUMENT", "keyId required");
            return;
          }
          auto path = KeyPathForId(key_id);
          if (!std::filesystem::exists(path)) {
            result->Success(flutter::EncodableValue());
            return;
          }
          std::vector<uint8_t> protected_data;
          std::string error;
          if (!ReadFileBytes(path, &protected_data, &error)) {
            result->Error("KEYSTORE_ERROR", error);
            return;
          }
          std::vector<uint8_t> plaintext;
          if (!UnprotectData(protected_data, &plaintext, &error)) {
            result->Error("KEYSTORE_ERROR", error);
            return;
          }
          result->Success(flutter::EncodableValue(plaintext));
          return;
        }

        if (method == "deleteKey") {
          std::string key_id;
          if (!get_string("keyId", &key_id)) {
            result->Error("INVALID_ARGUMENT", "keyId required");
            return;
          }
          std::error_code ec;
          std::filesystem::remove(KeyPathForId(key_id), ec);
          if (ec) {
            result->Error("KEYSTORE_ERROR", "Failed to delete key");
            return;
          }
          result->Success(flutter::EncodableValue(true));
          return;
        }

        if (method == "keyExists") {
          std::string key_id;
          if (!get_string("keyId", &key_id)) {
            result->Error("INVALID_ARGUMENT", "keyId required");
            return;
          }
          bool exists = std::filesystem::exists(KeyPathForId(key_id));
          result->Success(flutter::EncodableValue(exists));
          return;
        }

        if (method == "sealMasterKey") {
          std::vector<uint8_t> master_key;
          if (!get_bytes("masterKey", &master_key)) {
            result->Error("INVALID_ARGUMENT", "masterKey required");
            return;
          }
          std::vector<uint8_t> sealed;
          std::string error;
          if (!ProtectData(master_key, &sealed, &error)) {
            result->Error("SEAL_ERROR", error);
            return;
          }
          result->Success(flutter::EncodableValue(sealed));
          return;
        }

        if (method == "unsealMasterKey") {
          std::vector<uint8_t> sealed_key;
          if (!get_bytes("sealedKey", &sealed_key)) {
            result->Error("INVALID_ARGUMENT", "sealedKey required");
            return;
          }
          std::vector<uint8_t> unsealed;
          std::string error;
          if (!UnprotectData(sealed_key, &unsealed, &error)) {
            result->Error("UNSEAL_ERROR", error);
            return;
          }
          result->Success(flutter::EncodableValue(unsealed));
          return;
        }

        result->NotImplemented();
      });

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
