// Birthday Picker Screen - Select wallet birthday for sync optimization

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../config/endpoints.dart' as endpoints;
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';
import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../widgets/onboarding_progress_indicator.dart';
import '../../../core/i18n/arb_text_localizer.dart';

/// Birthday input mode
enum BirthdayInputMode { approxDate, exactHeight }

/// Birthday picker screen for wallet restoration
class BirthdayPickerScreen extends ConsumerStatefulWidget {
  const BirthdayPickerScreen({super.key});

  @override
  ConsumerState<BirthdayPickerScreen> createState() =>
      _BirthdayPickerScreenState();
}

class _BirthdayPickerScreenState extends ConsumerState<BirthdayPickerScreen> {
  static const int _blocksPerDay = 720;
  static const int _minYear = 2018;
  static final DateTime _pirateGenesisUtc = DateTime.utc(2018, 8, 29);
  static const List<String> _monthLabels = [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
  ];

  final _exactHeightController = TextEditingController();
  BirthdayInputMode _inputMode = BirthdayInputMode.approxDate;
  int _selectedMonth = DateTime.now().month;
  int _selectedYear = DateTime.now().year;
  int? _latestHeight;
  bool _loadingHeight = false;
  String? _heightError;
  bool _isCreating = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadLatestHeight();
  }

  @override
  void dispose() {
    _exactHeightController.dispose();
    super.dispose();
  }

  List<int> get _yearOptions {
    final nowYear = DateTime.now().year;
    final count = (nowYear - _minYear).clamp(0, 200);
    return List<int>.generate(count + 1, (i) => nowYear - i);
  }

  int? get _selectedHeight {
    if (_inputMode == BirthdayInputMode.exactHeight) {
      return int.tryParse(_exactHeightController.text.trim());
    }
    return _heightFromDate();
  }

  Future<bool> _canReadWalletRegistry() async {
    try {
      final wallets = await FfiBridge.listWallets();
      return wallets.isNotEmpty;
    } catch (_) {
      return false;
    }
  }

  int? _heightFromDate() {
    final selected = DateTime.utc(_selectedYear, _selectedMonth, 1);
    if (selected.isBefore(_pirateGenesisUtc)) {
      return 1;
    }

    final daysFromGenesis = selected.difference(_pirateGenesisUtc).inDays;
    final estimate = (daysFromGenesis * _blocksPerDay) + 1;
    if (_latestHeight != null) {
      return estimate.clamp(1, _latestHeight!);
    }
    return estimate < 1 ? 1 : estimate;
  }

  Future<void> _loadLatestHeight() async {
    setState(() {
      _loadingHeight = true;
      _heightError = null;
    });

    try {
      final result = await FfiBridge.testNode(url: endpoints.kDefaultLightd);
      if (!mounted) return;
      if (result.success && result.latestBlockHeight != null) {
        setState(() {
          _latestHeight = result.latestBlockHeight;
          _heightError = null;
        });
      } else {
        setState(() {
          _heightError =
              'Tor is still connecting. Latest network height will appear when available.';
        });
      }
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _heightError =
            'Tor is still connecting. Latest network height will appear when available.';
      });
    } finally {
      if (mounted) {
        setState(() => _loadingHeight = false);
      }
    }
  }

  Future<void> _completeSetup() async {
    final state = ref.read(onboardingControllerProvider);

    final hasAppPassphrase = await FfiBridge.hasAppPassphrase();
    if ((state.passphrase == null || state.passphrase!.isEmpty) &&
        !hasAppPassphrase) {
      setState(() => _error = 'Passphrase missing. Go back and set one.');
      return;
    }

    final selectedHeight = _selectedHeight;
    if (selectedHeight == null || selectedHeight <= 0) {
      setState(() => _error = 'Enter a valid block height.');
      return;
    }
    if (_latestHeight != null && selectedHeight > _latestHeight!) {
      setState(
        () => _error = 'Block height cannot be higher than the network tip.',
      );
      return;
    }

    setState(() {
      _isCreating = true;
      _error = null;
    });

    try {
      if (state.mode == OnboardingMode.create) {
        await FfiBridge.createWallet(
          name: 'My Pirate Wallet',
          entropyLen: 256,
          birthday: selectedHeight,
        );
      } else {
        if (state.mnemonic == null || state.mnemonic!.isEmpty) {
          throw StateError('Mnemonic not provided for restore');
        }

        await FfiBridge.restoreWallet(
          name: 'Restored Wallet',
          mnemonic: state.mnemonic!,
          birthday: selectedHeight,
        );
      }

      ref.read(onboardingControllerProvider.notifier)
        ..setBirthdayHeight(selectedHeight)
        ..nextStep();

      ref.invalidate(walletsExistProvider);
      final walletsExist = await ref.read(walletsExistProvider.future);
      if (!mounted) return;
      if (!walletsExist) {
        setState(() {
          _error = 'Wallet creation succeeded but was not detected. Try again.';
          _isCreating = false;
        });
        return;
      }
      final registryUnlocked = await _canReadWalletRegistry();
      if (!mounted) return;

      if (registryUnlocked) {
        ref.read(appUnlockedProvider.notifier).unlocked = true;
        context.go('/home');
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              state.mode == OnboardingMode.create
                  ? 'Wallet created. Syncing...'
                  : 'Wallet restored. Syncing from block ${_formatHeight(selectedHeight)}...',
            ),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
      } else {
        ref.read(appUnlockedProvider.notifier).unlocked = false;
        context.go('/unlock?redirect=/home');
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Wallet setup complete. Unlock to continue.'.tr),
            backgroundColor: AppColors.warning,
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    } catch (e) {
      setState(() {
        _error = 'Failed to create wallet: $e';
        _isCreating = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final onboardingState = ref.watch(onboardingControllerProvider);
    final isRestore = onboardingState.mode == OnboardingMode.import;
    final totalSteps = isRestore ? 5 : 6;
    final currentStep = isRestore ? 5 : 5;
    final gutter = AppSpacing.responsiveGutter(
      MediaQuery.of(context).size.width,
    );
    final viewInsets = MediaQuery.of(context).viewInsets.bottom;
    final selectedHeight = _selectedHeight;
    final tip = _latestHeight;
    final blocksToScan = (selectedHeight != null && tip != null)
        ? (tip - selectedHeight).clamp(0, tip)
        : null;

    return PScaffold(
      title: 'Birthday Picker'.tr,
      appBar: PAppBar(
        title: isRestore ? 'Wallet birthday' : 'Almost done',
        subtitle: isRestore
            ? 'Pick a start point to speed up sync'
            : 'Pick a start height for first sync',
        onBack: () => context.pop(),
      ),
      body: Column(
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              AppSpacing.lg,
              gutter,
              AppSpacing.lg,
            ),
            child: OnboardingProgressIndicator(
              currentStep: currentStep,
              totalSteps: totalSteps,
            ),
          ),
          Expanded(
            child: SingleChildScrollView(
              padding: EdgeInsets.fromLTRB(gutter, 0, gutter, viewInsets),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    isRestore
                        ? 'When did this wallet first transact?'
                        : 'Ready to create your wallet',
                    style: AppTypography.h2.copyWith(
                      color: AppColors.textPrimary,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  Text(
                    isRestore
                        ? 'Choose an approximate date or enter the exact block height.'
                        : 'We will start sync from a recent block height to speed up setup.',
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.lg),
                  PCard(
                    child: Padding(
                      padding: const EdgeInsets.all(AppSpacing.md),
                      child: Row(
                        children: [
                          Icon(Icons.public, color: AppColors.accentPrimary),
                          const SizedBox(width: AppSpacing.sm),
                          Expanded(
                            child: Text(
                              _loadingHeight
                                  ? 'Fetching latest block height...'
                                  : tip == null
                                  ? 'Latest network height unavailable while Tor connects'
                                  : 'Network tip: ${_formatHeight(tip)}',
                              style: AppTypography.body.copyWith(
                                color: AppColors.textPrimary,
                              ),
                            ),
                          ),
                          PTextButton(
                            label: _loadingHeight ? 'Loading' : 'Refresh',
                            onPressed: _loadingHeight
                                ? null
                                : _loadLatestHeight,
                            variant: PTextButtonVariant.subtle,
                          ),
                        ],
                      ),
                    ),
                  ),
                  if (_heightError != null) ...[
                    const SizedBox(height: AppSpacing.sm),
                    Text(
                      _heightError!,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                  const SizedBox(height: AppSpacing.lg),
                  _ModeCard(
                    mode: BirthdayInputMode.approxDate,
                    selected: _inputMode,
                    title: 'Approximate date'.tr,
                    subtitle: 'Month and year before your first transaction'.tr,
                    icon: Icons.calendar_today,
                    onTap: () => setState(
                      () => _inputMode = BirthdayInputMode.approxDate,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  _ModeCard(
                    mode: BirthdayInputMode.exactHeight,
                    selected: _inputMode,
                    title: 'Exact block height'.tr,
                    subtitle: 'Use the precise block height if you know it'.tr,
                    icon: Icons.pin_outlined,
                    onTap: () => setState(
                      () => _inputMode = BirthdayInputMode.exactHeight,
                    ),
                  ),
                  const SizedBox(height: AppSpacing.lg),
                  if (_inputMode == BirthdayInputMode.approxDate) ...[
                    Row(
                      children: [
                        Expanded(
                          child: DropdownMenuFormField<int>(
                            initialSelection: _selectedMonth,
                            label: Text('Month'.tr),
                            inputDecorationTheme: InputDecorationTheme(
                              filled: true,
                              fillColor: AppColors.surfaceElevated,
                            ),
                            dropdownMenuEntries: List.generate(
                              _monthLabels.length,
                              (index) => DropdownMenuEntry(
                                value: index + 1,
                                label: _monthLabels[index],
                              ),
                            ),
                            onSelected: (value) {
                              if (value == null) return;
                              setState(() => _selectedMonth = value);
                            },
                          ),
                        ),
                        const SizedBox(width: AppSpacing.md),
                        Expanded(
                          child: DropdownMenuFormField<int>(
                            initialSelection: _selectedYear,
                            label: Text('Year'.tr),
                            inputDecorationTheme: InputDecorationTheme(
                              filled: true,
                              fillColor: AppColors.surfaceElevated,
                            ),
                            dropdownMenuEntries: _yearOptions
                                .map(
                                  (year) => DropdownMenuEntry(
                                    value: year,
                                    label: year.toString(),
                                  ),
                                )
                                .toList(),
                            onSelected: (value) {
                              if (value == null) return;
                              setState(() => _selectedYear = value);
                            },
                          ),
                        ),
                      ],
                    ),
                  ] else ...[
                    PInput(
                      controller: _exactHeightController,
                      label: 'Block height'.tr,
                      hint: 'Enter the exact block height',
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    ),
                  ],
                  const SizedBox(height: AppSpacing.lg),
                  PCard(
                    child: Padding(
                      padding: const EdgeInsets.all(AppSpacing.md),
                      child: LayoutBuilder(
                        builder: (context, constraints) {
                          final isNarrow = constraints.maxWidth < 360;
                          final startBlock = Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                'Start block'.tr,
                                style: AppTypography.caption.copyWith(
                                  color: AppColors.textSecondary,
                                ),
                              ),
                              Text(
                                selectedHeight == null
                                    ? '--'
                                    : _formatHeight(selectedHeight),
                                style: AppTypography.h3.copyWith(
                                  color: AppColors.accentPrimary,
                                ),
                              ),
                            ],
                          );
                          final blocksScan = Column(
                            crossAxisAlignment: isNarrow
                                ? CrossAxisAlignment.start
                                : CrossAxisAlignment.end,
                            children: [
                              Text(
                                'Blocks to scan'.tr,
                                style: AppTypography.caption.copyWith(
                                  color: AppColors.textSecondary,
                                ),
                              ),
                              Text(
                                blocksToScan == null
                                    ? '--'
                                    : '~${_formatHeight(blocksToScan)}',
                                style: AppTypography.bodyBold.copyWith(
                                  color: AppColors.textPrimary,
                                ),
                              ),
                            ],
                          );
                          if (isNarrow) {
                            return Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                startBlock,
                                const SizedBox(height: AppSpacing.md),
                                blocksScan,
                              ],
                            );
                          }
                          return Row(
                            mainAxisAlignment: MainAxisAlignment.spaceBetween,
                            children: [startBlock, blocksScan],
                          );
                        },
                      ),
                    ),
                  ),
                  const SizedBox(height: AppSpacing.lg),
                  if (_error != null)
                    Container(
                      padding: const EdgeInsets.all(AppSpacing.md),
                      margin: const EdgeInsets.only(bottom: AppSpacing.lg),
                      decoration: BoxDecoration(
                        color: AppColors.error.withValues(alpha: 0.1),
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(
                          color: AppColors.error.withValues(alpha: 0.3),
                        ),
                      ),
                      child: Row(
                        children: [
                          Icon(
                            Icons.error_outline,
                            color: AppColors.error,
                            size: 20,
                          ),
                          const SizedBox(width: AppSpacing.sm),
                          Expanded(
                            child: Text(
                              _error!,
                              style: AppTypography.body.copyWith(
                                color: AppColors.error,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  Container(
                    padding: const EdgeInsets.all(AppSpacing.md),
                    decoration: BoxDecoration(
                      color: AppColors.accentPrimary.withValues(alpha: 0.1),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(
                        color: AppColors.accentPrimary.withValues(alpha: 0.3),
                      ),
                    ),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Icon(
                          Icons.info_outline,
                          color: AppColors.accentPrimary,
                          size: 20,
                        ),
                        const SizedBox(width: AppSpacing.sm),
                        Expanded(
                          child: Text(
                            isRestore
                                ? 'If you are unsure, choose an earlier start. '
                                      'Sync takes longer but avoids missing activity. '
                                      'Latest network height is optional during restore and will appear once Tor connects.'
                                : 'You can use the app while it syncs in the background.',
                            style: AppTypography.caption.copyWith(
                              color: AppColors.textPrimary,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: AppSpacing.xxl),
                ],
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(AppSpacing.lg),
            child: PButton(
              text: _isCreating
                  ? 'Creating...'
                  : (isRestore ? 'Restore wallet' : 'Create wallet'),
              onPressed: !_isCreating ? _completeSetup : null,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isCreating,
            ),
          ),
        ],
      ),
    );
  }

  String _formatHeight(int height) {
    return height.toString().replaceAllMapped(
      RegExp(r'(\\d{1,3})(?=(\\d{3})+(?!\\d))'),
      (m) => '${m[1]},',
    );
  }
}

class _ModeCard extends StatelessWidget {
  final BirthdayInputMode mode;
  final BirthdayInputMode selected;
  final String title;
  final String subtitle;
  final IconData icon;
  final VoidCallback onTap;

  const _ModeCard({
    required this.mode,
    required this.selected,
    required this.title,
    required this.subtitle,
    required this.icon,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final isSelected = mode == selected;
    return PCard(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(16),
        child: Container(
          padding: const EdgeInsets.all(AppSpacing.md),
          decoration: BoxDecoration(
            border: isSelected
                ? Border.all(color: AppColors.accentPrimary, width: 2)
                : null,
            borderRadius: BorderRadius.circular(16),
          ),
          child: Row(
            children: [
              Icon(
                icon,
                color: isSelected
                    ? AppColors.accentPrimary
                    : AppColors.textSecondary,
              ),
              const SizedBox(width: AppSpacing.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      style: AppTypography.bodyBold.copyWith(
                        color: AppColors.textPrimary,
                      ),
                    ),
                    Text(
                      subtitle,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
              RadioGroup<BirthdayInputMode>(
                groupValue: selected,
                onChanged: (_) => onTap(),
                child: Radio<BirthdayInputMode>(
                  value: mode,
                  activeColor: AppColors.accentPrimary,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
