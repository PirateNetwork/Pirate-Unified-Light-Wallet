import 'dart:convert';

import 'debug_log_writer.dart';

/// Writes one structured diagnostic event when debug logging is enabled.
Future<void> appendDebugEvent({
  required String id,
  required String message,
  StackTrace? stackTrace,
  Map<String, Object?> fields = const <String, Object?>{},
}) async {
  try {
    await appendDebugLogLine(
      jsonEncode(<String, Object?>{
        'id': id,
        'timestamp': DateTime.now().millisecondsSinceEpoch,
        'message': message,
        if (stackTrace != null) 'stack': stackTrace.toString(),
        ...fields,
      }),
    );
  } catch (_) {
    // Diagnostics must never interrupt the action being reported.
  }
}
