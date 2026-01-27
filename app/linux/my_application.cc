#include "my_application.h"

#include <flutter_linux/flutter_linux.h>
#include <glib.h>
#include <libsecret/secret.h>
#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif
#include <string.h>

#include "flutter/generated_plugin_registrant.h"

struct _MyApplication {
  GtkApplication parent_instance;
  char** dart_entrypoint_arguments;
  FlMethodChannel* keystore_channel;
  FlMethodChannel* security_channel;
};

G_DEFINE_TYPE(MyApplication, my_application, GTK_TYPE_APPLICATION)

namespace {
const char kKeystoreChannelName[] = "com.pirate.wallet/keystore";
const char kSecurityChannelName[] = "com.pirate.wallet/security";
const char kMasterKeyId[] = "pirate_wallet_master_key";

const SecretSchema kPirateKeystoreSchema = {
    "com.pirate.wallet.keystore",
    SECRET_SCHEMA_NONE,
    {{"key_id", SECRET_SCHEMA_ATTRIBUTE_STRING},
     {nullptr, static_cast<SecretSchemaAttributeType>(0)}}};

FlMethodResponse* error_response(const char* code, const char* message) {
  return FL_METHOD_RESPONSE(fl_method_error_response_new(code, message, nullptr));
}

bool extract_string_arg(FlValue* args, const char* key, const gchar** out) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr || fl_value_get_type(value) != FL_VALUE_TYPE_STRING) {
    return false;
  }
  *out = fl_value_get_string(value);
  return true;
}

bool extract_bytes_arg(FlValue* args,
                       const char* key,
                       const uint8_t** out,
                       size_t* length) {
  FlValue* value = fl_value_lookup_string(args, key);
  if (value == nullptr ||
      fl_value_get_type(value) != FL_VALUE_TYPE_UINT8_LIST) {
    return false;
  }
  *out = fl_value_get_uint8_list(value);
  *length = fl_value_get_length(value);
  return true;
}

bool store_secret(const gchar* key_id,
                  const uint8_t* data,
                  size_t length,
                  const char* label,
                  gchar** error_message) {
  GError* error = nullptr;
  g_autofree gchar* encoded =
      g_base64_encode(reinterpret_cast<const guchar*>(data), length);
  gboolean stored = secret_password_store_sync(
      &kPirateKeystoreSchema, SECRET_COLLECTION_DEFAULT, label, encoded,
      nullptr, &error, "key_id", key_id, nullptr);
  if (!stored) {
    if (error_message) {
      *error_message =
          error ? g_strdup(error->message) : g_strdup("Failed to store secret");
    }
    if (error) {
      g_error_free(error);
    }
    return false;
  }
  return true;
}

FlMethodResponse* handle_retrieve_secret(const gchar* key_id) {
  GError* error = nullptr;
  gchar* encoded = secret_password_lookup_sync(&kPirateKeystoreSchema, nullptr,
                                               &error, "key_id", key_id, nullptr);
  if (encoded == nullptr) {
    if (error) {
      g_autofree gchar* message = g_strdup(error->message);
      g_error_free(error);
      return error_response("KEYSTORE_ERROR", message);
    }
    return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
  }
  gsize decoded_length = 0;
  g_autofree guchar* decoded = g_base64_decode(encoded, &decoded_length);
  secret_password_free(encoded);
  g_autoptr(FlValue) value =
      fl_value_new_uint8_list(decoded, decoded_length);
  return FL_METHOD_RESPONSE(fl_method_success_response_new(value));
}

FlMethodResponse* handle_delete_secret(const gchar* key_id) {
  GError* error = nullptr;
  gboolean cleared = secret_password_clear_sync(&kPirateKeystoreSchema, nullptr,
                                                &error, "key_id", key_id,
                                                nullptr);
  if (!cleared && error != nullptr) {
    g_autofree gchar* message = g_strdup(error->message);
    g_error_free(error);
    return error_response("KEYSTORE_ERROR", message);
  }
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(true)));
}

FlMethodResponse* handle_key_exists(const gchar* key_id) {
  GError* error = nullptr;
  gchar* encoded = secret_password_lookup_sync(&kPirateKeystoreSchema, nullptr,
                                               &error, "key_id", key_id, nullptr);
  if (error) {
    g_autofree gchar* message = g_strdup(error->message);
    g_error_free(error);
    return error_response("KEYSTORE_ERROR", message);
  }
  bool exists = encoded != nullptr;
  if (encoded) {
    secret_password_free(encoded);
  }
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(exists)));
}

FlMethodResponse* handle_get_capabilities() {
  FlValue* map = fl_value_new_map();
  fl_value_set_string_take(map, "hasSecureHardware",
                           fl_value_new_bool(false));
  fl_value_set_string_take(map, "hasStrongBox", fl_value_new_bool(false));
  fl_value_set_string_take(map, "hasSecureEnclave",
                           fl_value_new_bool(false));
  fl_value_set_string_take(map, "hasBiometrics", fl_value_new_bool(false));
  return FL_METHOD_RESPONSE(fl_method_success_response_new(map));
}

static void keystore_method_call_handler(FlMethodChannel* channel,
                                         FlMethodCall* method_call,
                                         gpointer user_data) {
  const gchar* method = fl_method_call_get_name(method_call);
  FlValue* args = fl_method_call_get_args(method_call);

  g_autoptr(FlMethodResponse) response = nullptr;

  if (strcmp(method, "getCapabilities") == 0) {
    response = handle_get_capabilities();
  } else if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_MAP) {
    response = error_response("INVALID_ARGUMENT", "Arguments missing");
  } else if (strcmp(method, "storeKey") == 0) {
    const gchar* key_id = nullptr;
    const uint8_t* data = nullptr;
    size_t length = 0;
    if (!extract_string_arg(args, "keyId", &key_id) ||
        !extract_bytes_arg(args, "encryptedKey", &data, &length)) {
      response = error_response("INVALID_ARGUMENT",
                                "keyId and encryptedKey required");
    } else {
      g_autofree gchar* error_message = nullptr;
      if (!store_secret(key_id, data, length, "Pirate Wallet Key",
                        &error_message)) {
        response = error_response("KEYSTORE_ERROR",
                                  error_message ? error_message
                                                : "Failed to store key");
      } else {
        response = FL_METHOD_RESPONSE(
            fl_method_success_response_new(fl_value_new_bool(true)));
      }
    }
  } else if (strcmp(method, "retrieveKey") == 0) {
    const gchar* key_id = nullptr;
    if (!extract_string_arg(args, "keyId", &key_id)) {
      response = error_response("INVALID_ARGUMENT", "keyId required");
    } else {
      response = handle_retrieve_secret(key_id);
    }
  } else if (strcmp(method, "deleteKey") == 0) {
    const gchar* key_id = nullptr;
    if (!extract_string_arg(args, "keyId", &key_id)) {
      response = error_response("INVALID_ARGUMENT", "keyId required");
    } else {
      response = handle_delete_secret(key_id);
    }
  } else if (strcmp(method, "keyExists") == 0) {
    const gchar* key_id = nullptr;
    if (!extract_string_arg(args, "keyId", &key_id)) {
      response = error_response("INVALID_ARGUMENT", "keyId required");
    } else {
      response = handle_key_exists(key_id);
    }
  } else if (strcmp(method, "sealMasterKey") == 0) {
    const uint8_t* data = nullptr;
    size_t length = 0;
    if (!extract_bytes_arg(args, "masterKey", &data, &length)) {
      response = error_response("INVALID_ARGUMENT", "masterKey required");
    } else {
      g_autofree gchar* error_message = nullptr;
      if (!store_secret(kMasterKeyId, data, length,
                        "Pirate Wallet Master Key", &error_message)) {
        response = error_response("SEAL_ERROR",
                                  error_message ? error_message
                                                : "Failed to seal key");
      } else {
        g_autoptr(FlValue) empty_list = fl_value_new_list();
        response =
            FL_METHOD_RESPONSE(fl_method_success_response_new(empty_list));
      }
    }
  } else if (strcmp(method, "unsealMasterKey") == 0) {
    const uint8_t* unused = nullptr;
    size_t length = 0;
    if (!extract_bytes_arg(args, "sealedKey", &unused, &length)) {
      response = error_response("INVALID_ARGUMENT", "sealedKey required");
    } else {
      response = handle_retrieve_secret(kMasterKeyId);
    }
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }

  fl_method_call_respond(method_call, response, nullptr);
}

static void security_method_call_handler(FlMethodChannel* channel,
                                         FlMethodCall* method_call,
                                         gpointer user_data) {
  const gchar* method = fl_method_call_get_name(method_call);

  g_autoptr(FlMethodResponse) response = nullptr;
  if (strcmp(method, "enableScreenshotProtection") == 0 ||
      strcmp(method, "disableScreenshotProtection") == 0) {
    response = FL_METHOD_RESPONSE(
        fl_method_success_response_new(fl_value_new_bool(false)));
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }

  fl_method_call_respond(method_call, response, nullptr);
}
}  // namespace

// Implements GApplication::activate.
static void my_application_activate(GApplication* application) {
  MyApplication* self = MY_APPLICATION(application);
  GtkWindow* window =
      GTK_WINDOW(gtk_application_window_new(GTK_APPLICATION(application)));

  // Use a header bar when running in GNOME as this is the common style used
  // by applications and is the setup most users will be using (e.g. Ubuntu
  // desktop).
  // If running on X and not using GNOME then just use a traditional title bar
  // in case the window manager does more exotic layout, e.g. tiling.
  // If running on Wayland assume the header bar will work (may need changing
  // if future cases occur).
  gboolean use_header_bar = TRUE;
#ifdef GDK_WINDOWING_X11
  GdkScreen* screen = gtk_window_get_screen(window);
  if (GDK_IS_X11_SCREEN(screen)) {
    const gchar* wm_name = gdk_x11_screen_get_window_manager_name(screen);
    if (g_strcmp0(wm_name, "GNOME Shell") != 0) {
      use_header_bar = FALSE;
    }
  }
#endif
  if (use_header_bar) {
    GtkHeaderBar* header_bar = GTK_HEADER_BAR(gtk_header_bar_new());
    gtk_widget_show(GTK_WIDGET(header_bar));
    gtk_header_bar_set_title(header_bar, "app");
    gtk_header_bar_set_show_close_button(header_bar, TRUE);
    gtk_window_set_titlebar(window, GTK_WIDGET(header_bar));
  } else {
    gtk_window_set_title(window, "app");
  }

  gtk_window_set_default_size(window, 1280, 720);
  gtk_widget_show(GTK_WIDGET(window));

  g_autoptr(FlDartProject) project = fl_dart_project_new();
  fl_dart_project_set_dart_entrypoint_arguments(project, self->dart_entrypoint_arguments);

  FlView* view = fl_view_new(project);
  gtk_widget_show(GTK_WIDGET(view));
  gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));

  fl_register_plugins(FL_PLUGIN_REGISTRY(view));

  FlEngine* engine = fl_view_get_engine(view);
  FlBinaryMessenger* messenger = fl_engine_get_binary_messenger(engine);
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  self->keystore_channel = fl_method_channel_new(
      messenger, kKeystoreChannelName, FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(self->keystore_channel,
                                            keystore_method_call_handler, self,
                                            nullptr);

  self->security_channel = fl_method_channel_new(
      messenger, kSecurityChannelName, FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(self->security_channel,
                                            security_method_call_handler, self,
                                            nullptr);

  gtk_widget_grab_focus(GTK_WIDGET(view));
}

// Implements GApplication::local_command_line.
static gboolean my_application_local_command_line(GApplication* application, gchar*** arguments, int* exit_status) {
  MyApplication* self = MY_APPLICATION(application);
  // Strip out the first argument as it is the binary name.
  self->dart_entrypoint_arguments = g_strdupv(*arguments + 1);

  g_autoptr(GError) error = nullptr;
  if (!g_application_register(application, nullptr, &error)) {
     g_warning("Failed to register: %s", error->message);
     *exit_status = 1;
     return TRUE;
  }

  g_application_activate(application);
  *exit_status = 0;

  return TRUE;
}

// Implements GApplication::startup.
static void my_application_startup(GApplication* application) {
  //MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application startup.

  G_APPLICATION_CLASS(my_application_parent_class)->startup(application);
}

// Implements GApplication::shutdown.
static void my_application_shutdown(GApplication* application) {
  //MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application shutdown.

  G_APPLICATION_CLASS(my_application_parent_class)->shutdown(application);
}

// Implements GObject::dispose.
static void my_application_dispose(GObject* object) {
  MyApplication* self = MY_APPLICATION(object);
  g_clear_object(&self->keystore_channel);
  g_clear_object(&self->security_channel);
  g_clear_pointer(&self->dart_entrypoint_arguments, g_strfreev);
  G_OBJECT_CLASS(my_application_parent_class)->dispose(object);
}

static void my_application_class_init(MyApplicationClass* klass) {
  G_APPLICATION_CLASS(klass)->activate = my_application_activate;
  G_APPLICATION_CLASS(klass)->local_command_line = my_application_local_command_line;
  G_APPLICATION_CLASS(klass)->startup = my_application_startup;
  G_APPLICATION_CLASS(klass)->shutdown = my_application_shutdown;
  G_OBJECT_CLASS(klass)->dispose = my_application_dispose;
}

static void my_application_init(MyApplication* self) {
  self->keystore_channel = nullptr;
  self->security_channel = nullptr;
}

MyApplication* my_application_new() {
  return MY_APPLICATION(g_object_new(my_application_get_type(),
                                     "application-id", APPLICATION_ID,
                                     "flags", G_APPLICATION_NON_UNIQUE,
                                     nullptr));
}
