import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:window_manager/window_manager.dart';

import '../../features/settings/providers/preferences_providers.dart';
import '../../l10n/app_localizations.dart';
import '../../ui/molecules/p_snack.dart';
import '../services/desktop_update_service.dart';

class DesktopUpdatePromptHost extends ConsumerStatefulWidget {
  const DesktopUpdatePromptHost({required this.child, super.key});

  final Widget child;

  @override
  ConsumerState<DesktopUpdatePromptHost> createState() =>
      _DesktopUpdatePromptHostState();
}

class _DesktopUpdatePromptHostState
    extends ConsumerState<DesktopUpdatePromptHost> {
  static const String _dismissedTagStorageKey =
      'ui_update_prompt_dismissed_tag_v1';
  static const Duration _initialDelay = Duration(seconds: 25);
  static const Duration _pollInterval = Duration(minutes: 30);

  final FlutterSecureStorage _storage = const FlutterSecureStorage();
  Timer? _initialTimer;
  Timer? _periodicTimer;
  bool _checkInProgress = false;
  bool _dialogVisible = false;

  bool get _isDesktop =>
      !kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS);

  @override
  void initState() {
    super.initState();
    if (!_isDesktop) {
      return;
    }
    _initialTimer = Timer(_initialDelay, _runCheck);
    _periodicTimer = Timer.periodic(_pollInterval, (_) => _runCheck());
  }

  @override
  void dispose() {
    _initialTimer?.cancel();
    _periodicTimer?.cancel();
    super.dispose();
  }

  Future<void> _runCheck() async {
    if (!mounted || _checkInProgress || _dialogVisible) {
      return;
    }
    final allowChecks = ref.read(allowDesktopUpdateApisProvider);
    if (!allowChecks) {
      return;
    }

    _checkInProgress = true;
    try {
      final candidate = await DesktopUpdateService.instance
          .checkForCurrentVersionUpdate();
      if (!mounted || candidate == null) {
        return;
      }

      final dismissedTag = await _storage.read(key: _dismissedTagStorageKey);
      if (dismissedTag != null && dismissedTag == candidate.release.tagName) {
        return;
      }

      await _showUpdateDialog(candidate);
    } catch (e) {
      debugPrint('Desktop update check failed: $e');
    } finally {
      _checkInProgress = false;
    }
  }

  Future<void> _showUpdateDialog(DesktopUpdateCandidate candidate) async {
    _dialogVisible = true;
    final context = this.context;
    final l10n = AppLocalizations.of(context);
    final displayVersion = candidate.release.tagName.isEmpty
        ? candidate.release.name
        : candidate.release.tagName;
    final publishedAt = candidate.release.publishedAt?.toLocal();
    final publishedText = publishedAt == null
        ? l10n.unknownPublishTime
        : '${publishedAt.year.toString().padLeft(4, '0')}-'
              '${publishedAt.month.toString().padLeft(2, '0')}-'
              '${publishedAt.day.toString().padLeft(2, '0')} '
              '${publishedAt.hour.toString().padLeft(2, '0')}:'
              '${publishedAt.minute.toString().padLeft(2, '0')}';

    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) {
        return AlertDialog(
          title: Text(l10n.updateAvailableTitle),
          content: Text(
            l10n.updateAvailableMessage(displayVersion, publishedText),
          ),
          actions: [
            FilledButton(
              onPressed: () async {
                if (dialogContext.mounted) {
                  Navigator.of(dialogContext).pop();
                }
                await _downloadAndInstall(candidate);
              },
              child: Text(l10n.updateButton),
            ),
            TextButton(
              onPressed: () async {
                final url = Uri.tryParse(candidate.release.releaseUrl);
                if (url != null) {
                  await launchUrl(url, mode: LaunchMode.externalApplication);
                }
                if (dialogContext.mounted) {
                  Navigator.of(dialogContext).pop();
                }
              },
              child: Text(l10n.changelogButton),
            ),
            TextButton(
              onPressed: () async {
                await _storage.write(
                  key: _dismissedTagStorageKey,
                  value: candidate.release.tagName,
                );
                if (dialogContext.mounted) {
                  Navigator.of(dialogContext).pop();
                }
              },
              child: Text(l10n.cancelButton),
            ),
          ],
        );
      },
    );
    _dialogVisible = false;
  }

  Future<void> _downloadAndInstall(DesktopUpdateCandidate candidate) async {
    if (!mounted) {
      return;
    }
    final l10n = AppLocalizations.of(context);

    unawaited(
      showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (dialogContext) {
          return const AlertDialog(
            content: Row(
              children: [
                SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2.2),
                ),
                SizedBox(width: 12),
                Expanded(child: _PreparingUpdateLabel()),
              ],
            ),
          );
        },
      ),
    );

    try {
      await DesktopUpdateService.instance.launchUpdate(candidate);
      if (mounted) {
        Navigator.of(context, rootNavigator: true).pop();
      }
      if (!mounted) {
        return;
      }
      PSnack.show(
        context: context,
        message: l10n.installerLaunchedClosing,
        variant: PSnackVariant.success,
      );
      await Future<void>.delayed(const Duration(milliseconds: 450));
      await _closeAppForUpdate();
    } catch (e) {
      if (mounted) {
        Navigator.of(context, rootNavigator: true).pop();
      }
      if (mounted) {
        PSnack.show(
          context: context,
          message: l10n.automaticUpdateFailedMessage(e.toString()),
          variant: PSnackVariant.error,
          duration: const Duration(seconds: 6),
        );
      }
    }
  }

  Future<void> _closeAppForUpdate() async {
    if (!_isDesktop) {
      return;
    }
    try {
      await windowManager.close();
      return;
    } catch (_) {
      // Fall back to process exit.
    }
    exit(0);
  }

  @override
  Widget build(BuildContext context) {
    return widget.child;
  }
}

class _PreparingUpdateLabel extends StatelessWidget {
  const _PreparingUpdateLabel();

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Text(l10n.preparingUpdateInstaller);
  }
}
