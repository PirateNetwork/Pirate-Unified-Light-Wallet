/// Settings screen - Wallet configuration
library;

import 'dart:io';
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:package_info_plus/package_info_plus.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../l10n/app_localizations.dart';
import 'providers/preferences_providers.dart';
import 'providers/transport_providers.dart';
import '../../ui/molecules/p_list_tile.dart';
import '../../ui/molecules/connection_status_indicator.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/logging/debug_log_path.dart';
import '../../core/i18n/arb_text_localizer.dart';

final appVersionProvider = FutureProvider<String>((ref) async {
  final info = await PackageInfo.fromPlatform();
  final version = info.version.trim();
  if (version.isEmpty) {
    return 'Unknown';
  }
  return 'v$version';
});

/// Settings screen
class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  static Future<void> _appendRescanLog(String message) async {
    try {
      final logPath = await resolveDebugLogPath();
      final file = File(logPath);
      final ts = DateTime.now().millisecondsSinceEpoch;
      final line =
          '{"id":"log_dart_rescan","timestamp":$ts,"message":"$message"}\n';
      await file.writeAsString(line, mode: FileMode.append, flush: true);
    } catch (_) {
      // Ignore logging failures.
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final size = MediaQuery.of(context).size;
    final screenWidth = size.width;
    final isMobile = AppSpacing.isMobile(screenWidth);
    final isDesktop = AppSpacing.isDesktop(screenWidth);
    final content = ListView(
      padding: EdgeInsets.zero,
      children: [
        _SettingsSection(
          title: 'Security'.tr,
          topPadding: isMobile ? AppSpacing.lg : AppSpacing.xl,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final enabled = ref.watch(biometricsEnabledProvider);
                final resolved = ref.watch(resolvedBiometricsEnabledProvider);
                final availability = ref.watch(biometricAvailabilityProvider);
                final subtitle = resolved.when(
                  data: (_) => availability.when(
                    data: (available) {
                      if (!available) return 'Unavailable';
                      return enabled ? 'On' : 'Off';
                    },
                    loading: () => 'Checking...',
                    error: (_, _) => enabled ? 'On' : 'Off',
                  ),
                  loading: () => 'Checking...',
                  error: (_, _) => enabled ? 'On' : 'Off',
                );
                return PListTile(
                  leading: const Icon(Icons.fingerprint),
                  title: 'Biometrics'.tr,
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/biometrics'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            PListTile(
              leading: const Icon(Icons.lock_reset_outlined),
              title: 'Change passphrase'.tr,
              subtitle: 'Update your app unlock passphrase'.tr,
              onTap: () => context.push('/settings/passphrase'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: Icon(Icons.emergency, color: AppColors.warning),
              title: 'Duress passphrase'.tr,
              subtitle: 'Decoy wallet access'.tr,
              onTap: () => context.push('/settings/panic-pin'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        _SettingsSection(
          title: 'Privacy and Network'.tr,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final endpointAsync = ref.watch(lightdEndpointConfigProvider);
                final subtitle = endpointAsync.when(
                  data: (config) => config.displayString,
                  loading: () => 'Loading...',
                  error: (_, _) => 'lightd1.piratechain.com:9067',
                );
                return PListTile(
                  leading: const Icon(Icons.dns_outlined),
                  title: 'Node'.tr,
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/node-picker'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            Consumer(
              builder: (context, ref, _) {
                final config = ref.watch(transportConfigProvider);
                final subtitle = switch (config.mode) {
                  'tor' => 'Current: Tor',
                  'direct' => 'Current: Direct',
                  'socks5' => 'Current: SOCKS5',
                  'i2p' => 'Current: I2P',
                  _ => 'Current: ${config.mode}',
                };
                return PListTile(
                  leading: const Icon(Icons.shield_outlined),
                  title: 'Transport'.tr,
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/privacy-shield'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            PListTile(
              leading: const Icon(Icons.wifi_tethering_off_outlined),
              title: 'Outbound API Calls'.tr,
              subtitle: 'Control non-lightserver requests'.tr,
              onTap: () => context.push('/settings/outbound-apis'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        _SettingsSection(
          title: 'Backups'.tr,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final wallet = ref.watch(activeWalletMetaProvider);
                return PListTile(
                  leading: Icon(Icons.key_outlined, color: AppColors.warning),
                  title: 'Backup seed phrase'.tr,
                  subtitle: wallet == null
                      ? 'No active wallet'
                      : 'View your recovery phrase',
                  onTap: wallet == null
                      ? null
                      : () => context.push(
                          '/settings/export-seed'
                          '?walletId=${wallet.id}'
                          '&walletName=${Uri.encodeComponent(wallet.name)}',
                        ),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'Wallet'.tr,
          children: [
            PListTile(
              leading: const Icon(Icons.vpn_key_outlined),
              title: 'Keys & addresses'.tr,
              subtitle: 'Manage imported keys and addresses'.tr,
              onTap: () => context.push('/settings/keys'),
              trailing: const Icon(Icons.chevron_right),
            ),
            Consumer(
              builder: (context, ref, _) {
                final walletId = ref.watch(activeWalletProvider);
                final enabledAsync = ref.watch(
                  autoConsolidationEnabledProvider,
                );
                Widget buildTile({
                  required bool enabled,
                  required bool loading,
                }) {
                  final status = enabled ? 'On' : 'Off';
                  final subtitle = walletId == null
                      ? 'No active wallet'
                      : loading
                      ? 'Loading...'
                      : '$status - Combine unlabeled notes during sends';
                  return PListTile(
                    leading: const Icon(Icons.merge_type_outlined),
                    title: 'Auto consolidation'.tr,
                    subtitle: subtitle,
                    trailing: Switch(
                      value: enabled,
                      onChanged: walletId == null || loading
                          ? null
                          : (value) async {
                              await FfiBridge.setAutoConsolidationEnabled(
                                walletId,
                                value,
                              );
                              ref.invalidate(autoConsolidationEnabledProvider);
                            },
                    ),
                  );
                }

                return enabledAsync.when(
                  data: (enabled) =>
                      buildTile(enabled: enabled, loading: false),
                  loading: () => buildTile(enabled: false, loading: true),
                  error: (_, _) => buildTile(enabled: false, loading: false),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'Appearance'.tr,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final themeMode = ref.watch(appThemeModeProvider);
                return PListTile(
                  leading: const Icon(Icons.dark_mode_outlined),
                  title: 'Theme'.tr,
                  subtitle: themeMode.label,
                  onTap: () => context.push('/settings/theme'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            Consumer(
              builder: (context, ref, _) {
                final currency = ref.watch(currencyPreferenceProvider);
                return PListTile(
                  leading: const Icon(Icons.currency_bitcoin),
                  title: 'Currency'.tr,
                  subtitle: currency.code,
                  onTap: () => context.push('/settings/currency'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            Consumer(
              builder: (context, ref, _) {
                final locale = ref.watch(localePreferenceProvider);
                final subtitle = locale == AppLocalePreference.english
                    ? l10n.englishLanguage
                    : locale.label;
                return PListTile(
                  leading: const Icon(Icons.language_outlined),
                  title: l10n.languageSettingTitle,
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/language'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'Advanced'.tr,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final meta = ref.watch(activeWalletMetaProvider);
                final subtitle = meta == null
                    ? 'Not set'
                    : 'Block ${_formatHeight(meta.birthdayHeight)}';
                return PListTile(
                  leading: const Icon(Icons.cake_outlined),
                  title: 'Birthday height'.tr,
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/birthday-height'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            Consumer(
              builder: (context, ref, _) {
                return PListTile(
                  leading: const Icon(Icons.refresh_outlined),
                  title: 'Rescan blockchain'.tr,
                  subtitle: 'Rebuild wallet state'.tr,
                  onTap: () {
                    _showRescanDialog(context, ref);
                  },
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'About'.tr,
          children: [
            Consumer(
              builder: (context, ref, _) {
                final versionAsync = ref.watch(appVersionProvider);
                final subtitle = versionAsync.when(
                  data: (value) => value,
                  loading: () => 'Loading...',
                  error: (_, _) => 'Unknown',
                );
                return PListTile(
                  leading: const Icon(Icons.info_outlined),
                  title: 'Version'.tr,
                  subtitle: subtitle,
                  trailing: null,
                );
              },
            ),
            PListTile(
              leading: const Icon(Icons.verified_user),
              title: 'Verify build'.tr,
              subtitle: 'Reproducible build check'.tr,
              onTap: () => context.push('/settings/verify-build'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: const Icon(Icons.article_outlined),
              title: 'Terms and privacy'.tr,
              onTap: () => context.push('/settings/terms'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: const Icon(Icons.code_outlined),
              title: 'Open source licenses'.tr,
              onTap: () => context.push('/settings/licenses'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        const SizedBox(height: AppSpacing.xxl),
      ],
    );

    final appBarActions = [
      ConnectionStatusIndicator(
        full: !isMobile,
        onTap: () => context.push('/settings/privacy-shield'),
      ),
      if (!isMobile) const WalletSwitcherButton(compact: true),
    ];

    if (!useScaffold) {
      if (isDesktop) {
        return content;
      }
      return PScaffold(
        title: l10n.settingsTitle,
        useSafeArea: false,
        appBar: PAppBar(
          title: l10n.settingsTitle,
          subtitle: l10n.settingsSubtitle,
          actions: appBarActions,
        ),
        body: content,
      );
    }

    return PScaffold(
      title: l10n.settingsTitle,
      appBar: isDesktop
          ? null
          : PAppBar(
              title: l10n.settingsTitle,
              subtitle: l10n.settingsSubtitle,
              actions: appBarActions,
            ),
      body: content,
    );
  }

  Future<void> _showRescanDialog(BuildContext context, WidgetRef ref) async {
    try {
      debugPrint('_showRescanDialog called');
      int? suggestedHeight;
      bool appliedSuggested = false;
      if (!context.mounted) {
        debugPrint('Context not mounted before showing dialog');
        return;
      }
      final controller = TextEditingController(text: '1');
      final suggestedFuture = ref
          .read(lastCheckpointProvider.future)
          .timeout(
            const Duration(seconds: 2),
            onTimeout: () {
              debugPrint('Checkpoint loading timed out');
              return null;
            },
          )
          .catchError((Object e) {
            debugPrint('Error loading checkpoint: $e');
            return null;
          });

      debugPrint('Showing rescan dialog');
      final confirmed = await showDialog<bool>(
        context: context,
        barrierDismissible: true,
        builder: (dialogContext) => AlertDialog(
          backgroundColor: AppColors.surface,
          title: Text('Rescan Blockchain'.tr),
          content: FutureBuilder(
            future: suggestedFuture,
            builder: (context, snapshot) {
              final isLoading =
                  snapshot.connectionState == ConnectionState.waiting;
              if (!isLoading && snapshot.hasData) {
                suggestedHeight = snapshot.data?.height;
                if (!appliedSuggested &&
                    suggestedHeight != null &&
                    (controller.text.trim().isEmpty ||
                        controller.text.trim() == '1')) {
                  appliedSuggested = true;
                  WidgetsBinding.instance.addPostFrameCallback((_) {
                    if (!dialogContext.mounted) {
                      return;
                    }
                    controller.text = suggestedHeight.toString();
                  });
                }
              }
              final helperText = suggestedHeight == null
                  ? 'Enter a block height to rescan from.'
                  : 'Suggested: $suggestedHeight';
              return Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'This will rebuild wallet state and may take a while.'.tr,
                  ),
                  const SizedBox(height: AppSpacing.md),
                  TextField(
                    controller: controller,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    decoration: InputDecoration(
                      labelText: 'Start height'.tr,
                      hintText: 'e.g., 1'.tr,
                      helperText: helperText,
                    ),
                  ),
                ],
              );
            },
          ),
          actions: [
            TextButton(
              onPressed: () {
                controller.dispose();
                Navigator.of(dialogContext).pop(false);
              },
              child: Text('Cancel'.tr),
            ),
            TextButton(
              onPressed: () {
                Navigator.of(dialogContext).pop(true);
              },
              child: Text('Rescan'.tr),
            ),
          ],
        ),
      );

      if (confirmed ?? false) {
        final fromHeight = int.tryParse(controller.text.trim());
        if (fromHeight == null || fromHeight <= 0) {
          if (context.mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text('Enter a valid block height to rescan from.'.tr),
                backgroundColor: AppColors.error,
              ),
            );
          }
          controller.dispose();
          return;
        }
        debugPrint('Rescan confirmed, starting from height: $fromHeight');
        await _appendRescanLog('rescan requested from_height=$fromHeight');
        try {
          // Invalidate sync progress stream before rescan so home screen picks it up
          ref.invalidate(syncProgressStreamProvider);
          unawaited(
            ref
                .read(rescanProvider)(fromHeight)
                .then(
                  (_) => _appendRescanLog(
                    'rescan call completed from_height=$fromHeight',
                  ),
                )
                .catchError((Object e) async {
                  await _appendRescanLog(
                    'rescan call failed from_height=$fromHeight error=$e',
                  );
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(
                        content: Text('Failed to start rescan: $e'),
                        backgroundColor: AppColors.error,
                      ),
                    );
                  }
                }),
          );
          if (context.mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text('Rescan started from block $fromHeight'),
                backgroundColor: AppColors.success,
              ),
            );
          }
        } catch (e, stackTrace) {
          debugPrint('Error starting rescan: $e');
          debugPrint('Stack trace: $stackTrace');
          await _appendRescanLog(
            'rescan call failed from_height=$fromHeight error=$e',
          );
          if (context.mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text('Failed to start rescan: $e'),
                backgroundColor: AppColors.error,
              ),
            );
          }
        }
      } else {
        debugPrint('Rescan cancelled');
      }

      controller.dispose();
    } catch (e, stackTrace) {
      debugPrint('Error in _showRescanDialog: $e');
      debugPrint('Stack trace: $stackTrace');
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Error showing rescan dialog: $e'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  String _formatHeight(int height) {
    return height.toString().replaceAllMapped(
      RegExp(r'(\\d{1,3})(?=(\\d{3})+(?!\\d))'),
      (m) => '${m[1]},',
    );
  }
}

/// Settings section widget
class _SettingsSection extends StatelessWidget {
  final String title;
  final List<Widget> children;
  final double? topPadding;

  const _SettingsSection({
    required this.title,
    required this.children,
    this.topPadding,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(
            AppSpacing.lg,
            topPadding ?? AppSpacing.xl,
            AppSpacing.lg,
            AppSpacing.md,
          ),
          child: Text(
            title,
            style: AppTypography.caption.copyWith(
              color: AppColors.textSecondary,
              fontWeight: FontWeight.w600,
              letterSpacing: 1.2,
            ),
          ),
        ),
        ...children,
      ],
    );
  }
}
