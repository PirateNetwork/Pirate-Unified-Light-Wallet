/// Settings screen - Wallet configuration
library;

import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../design/deep_space_theme.dart';
import '../../core/ffi/ffi_bridge.dart';
import '../../core/providers/wallet_providers.dart';
import '../../design/tokens/colors.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import 'providers/preferences_providers.dart';
import '../../ui/molecules/p_list_tile.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';

/// Settings screen
class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key, this.useScaffold = true});

  final bool useScaffold;

  static Future<void> _appendRescanLog(String message) async {
    try {
      if (!Platform.isWindows) return;
      final logPath = Platform.environment['PIRATE_DEBUG_LOG_PATH'] ??
          '${Directory.current.path}${Platform.pathSeparator}.cursor${Platform.pathSeparator}debug.log';
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
    final content = ListView(
      children: [
        _SettingsSection(
          title: 'Security',
          children: [
            Consumer(
              builder: (context, ref, _) {
                final enabled = ref.watch(biometricsEnabledProvider);
                final availability = ref.watch(biometricAvailabilityProvider);
                final subtitle = availability.when(
                  data: (available) {
                    if (!available) return 'Unavailable';
                    return enabled ? 'On' : 'Off';
                  },
                  loading: () => 'Checking...',
                  error: (_, __) => enabled ? 'On' : 'Off',
                );
                return PListTile(
                  leading: const Icon(Icons.fingerprint),
                  title: 'Biometrics',
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/biometrics'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            PListTile(
              leading: const Icon(Icons.lock_reset_outlined),
              title: 'Change passphrase',
              subtitle: 'Update your app unlock passphrase',
              onTap: () => context.push('/settings/passphrase'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: Icon(Icons.emergency, color: AppColors.warning),
              title: 'Panic PIN',
              subtitle: 'Decoy wallet access',
              onTap: () => context.push('/settings/panic-pin'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        _SettingsSection(
          title: 'Privacy and Network',
          children: [
            Consumer(
              builder: (context, ref, _) {
                final endpointAsync = ref.watch(lightdEndpointConfigProvider);
                final subtitle = endpointAsync.when(
                  data: (config) => config.displayString,
                  loading: () => 'Loading...',
                  error: (_, __) => 'lightd1.piratechain.com:9067',
                );
                return PListTile(
                  leading: const Icon(Icons.dns_outlined),
                  title: 'Node',
                  subtitle: subtitle,
                  onTap: () => context.push('/settings/node-picker'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
            PListTile(
              leading: const Icon(Icons.shield_outlined),
              title: 'Transport',
              subtitle: 'Tor (recommended)',
              onTap: () => context.push('/settings/privacy-shield'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        _SettingsSection(
          title: 'Backups',
          children: [
            Consumer(
              builder: (context, ref, _) {
                final wallet = ref.watch(activeWalletMetaProvider);
                return PListTile(
                  leading: Icon(Icons.key_outlined, color: AppColors.warning),
                  title: 'Backup words',
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
          title: 'Wallet',
          children: [
            PListTile(
              leading: const Icon(Icons.vpn_key_outlined),
              title: 'Keys & addresses',
              subtitle: 'Manage imported keys and addresses',
              onTap: () => context.push('/settings/keys'),
              trailing: const Icon(Icons.chevron_right),
            ),
            Consumer(
              builder: (context, ref, _) {
                final walletId = ref.watch(activeWalletProvider);
                final enabledAsync = ref.watch(autoConsolidationEnabledProvider);
                Widget buildTile({required bool enabled, required bool loading}) {
                  final status = enabled ? 'On' : 'Off';
                  final subtitle = walletId == null
                      ? 'No active wallet'
                      : loading
                          ? 'Loading...'
                          : '$status - Combine unlabeled notes during sends';
                  return PListTile(
                    leading: const Icon(Icons.merge_type_outlined),
                    title: 'Auto consolidation',
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
                  data: (enabled) => buildTile(enabled: enabled, loading: false),
                  loading: () => buildTile(enabled: false, loading: true),
                  error: (_, __) => buildTile(enabled: false, loading: false),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'Appearance',
          children: [
            Consumer(
              builder: (context, ref, _) {
                final themeMode = ref.watch(appThemeModeProvider);
                return PListTile(
                  leading: const Icon(Icons.dark_mode_outlined),
                  title: 'Theme',
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
                  title: 'Currency',
                  subtitle: currency.code,
                  onTap: () => context.push('/settings/currency'),
                  trailing: const Icon(Icons.chevron_right),
                );
              },
            ),
          ],
        ),

        _SettingsSection(
          title: 'Advanced',
          children: [
            Consumer(
              builder: (context, ref, _) {
                final meta = ref.watch(activeWalletMetaProvider);
                final subtitle = meta == null
                    ? 'Not set'
                    : 'Block ${_formatHeight(meta.birthdayHeight)}';
                return PListTile(
                  leading: const Icon(Icons.cake_outlined),
                  title: 'Birthday height',
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
                  title: 'Rescan blockchain',
                  subtitle: 'Rebuild wallet state',
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
          title: 'About',
          children: [
            PListTile(
              leading: const Icon(Icons.info_outlined),
              title: 'Version',
              subtitle: '1.0.0-beta',
              trailing: null,
            ),
            PListTile(
              leading: const Icon(Icons.verified_user),
              title: 'Verify build',
              subtitle: 'Reproducible build check',
              onTap: () => context.push('/settings/verify-build'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: const Icon(Icons.article_outlined),
              title: 'Terms and privacy',
              onTap: () => context.push('/settings/terms'),
              trailing: const Icon(Icons.chevron_right),
            ),
            PListTile(
              leading: const Icon(Icons.code_outlined),
              title: 'Open source licenses',
              onTap: () => context.push('/settings/licenses'),
              trailing: const Icon(Icons.chevron_right),
            ),
          ],
        ),

        const SizedBox(height: AppSpacing.xxl),
      ],
    );

    if (!useScaffold) {
      return Column(
        children: [
          const PAppBar(
            title: 'Settings',
            subtitle: 'Security and privacy controls.',
            actions: [WalletSwitcherButton(compact: true)],
          ),
          Expanded(child: content),
        ],
      );
    }

    return PScaffold(
      title: 'Settings',
      appBar: const PAppBar(
        title: 'Settings',
        subtitle: 'Security and privacy controls.',
        actions: [WalletSwitcherButton(compact: true)],
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
          .catchError((e) {
        debugPrint('Error loading checkpoint: $e');
        return null;
      });

      debugPrint('Showing rescan dialog');
      final confirmed = await showDialog<bool>(
        context: context,
        barrierDismissible: true,
        builder: (dialogContext) => AlertDialog(
          backgroundColor: AppColors.surface,
          title: const Text('Rescan Blockchain'),
          content: FutureBuilder(
            future: suggestedFuture,
            builder: (context, snapshot) {
              final isLoading = snapshot.connectionState == ConnectionState.waiting;
              if (!isLoading && snapshot.hasData) {
                suggestedHeight = snapshot.data?.height;
                if (!appliedSuggested &&
                    suggestedHeight != null &&
                    (controller.text.trim().isEmpty || controller.text.trim() == '1')) {
                  appliedSuggested = true;
                  WidgetsBinding.instance.addPostFrameCallback((_) {
                    if (!dialogContext.mounted) {
                      return;
                    }
                    controller.text = suggestedHeight.toString();
                  });
                }
              }
              final helperText = isLoading
                  ? 'Loading suggested height...'
                  : (suggestedHeight == null
                      ? 'Enter a block height to rescan from.'
                      : 'Suggested: $suggestedHeight');
              return Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text(
                    'This will rebuild wallet state and may take a while.',
                  ),
                  const SizedBox(height: AppSpacing.md),
                  TextField(
                    controller: controller,
                    keyboardType: TextInputType.number,
                    decoration: InputDecoration(
                      labelText: 'Start height',
                      hintText: 'e.g., 1',
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
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () {
                Navigator.of(dialogContext).pop(true);
              },
              child: const Text('Rescan'),
            ),
          ],
        ),
      );

      if (confirmed == true) {
        final fromHeight = int.tryParse(controller.text.trim()) ?? 1;
        debugPrint('Rescan confirmed, starting from height: $fromHeight');
        await _appendRescanLog('rescan requested from_height=$fromHeight');
        try {
          // Invalidate sync progress stream before rescan so home screen picks it up
          ref.invalidate(syncProgressStreamProvider);
          ref
              .read(rescanProvider)(fromHeight)
              .then((_) => _appendRescanLog(
                    'rescan call completed from_height=$fromHeight',
                  ))
              .catchError((e) async {
            await _appendRescanLog(
              'rescan call failed from_height=$fromHeight error=$e',
            );
            if (context.mounted) {
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                  content: Text('Failed to start rescan: ${e.toString()}'),
                  backgroundColor: AppColors.error,
                ),
              );
            }
          });
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
                content: Text('Failed to start rescan: ${e.toString()}'),
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
            content: Text('Error showing rescan dialog: ${e.toString()}'),
            backgroundColor: AppColors.error,
          ),
        );
      }
    }
  }

  String _formatHeight(int height) {
    return height.toString().replaceAllMapped(
      RegExp(r'(\\d{1,3})(?=(\\d{3})+(?!\\d))'),
      (Match m) => '${m[1]},',
    );
  }
}

/// Settings section widget
class _SettingsSection extends StatelessWidget {
  final String title;
  final List<Widget> children;

  const _SettingsSection({
    required this.title,
    required this.children,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            AppSpacing.lg,
            AppSpacing.xl,
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
