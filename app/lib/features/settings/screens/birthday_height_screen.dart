/// Birthday height settings screen
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/ffi/generated/models.dart' show WalletMeta;
import '../../../core/providers/wallet_providers.dart';
import '../../../design/deep_space_theme.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_input.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/molecules/p_card.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

enum BirthdayHeightInputMode {
  approxDate,
  exactHeight,
}

class BirthdayHeightScreen extends ConsumerStatefulWidget {
  const BirthdayHeightScreen({super.key});

  @override
  ConsumerState<BirthdayHeightScreen> createState() => _BirthdayHeightScreenState();
}

class _BirthdayHeightScreenState extends ConsumerState<BirthdayHeightScreen> {
  static const int _blocksPerDay = 720;
  static const int _minYear = 2018;
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
  BirthdayHeightInputMode _inputMode = BirthdayHeightInputMode.exactHeight;
  int _selectedMonth = DateTime.now().month;
  int _selectedYear = DateTime.now().year;
  int? _latestHeight;
  bool _loadingHeight = false;
  bool _isSaving = false;
  String? _heightError;
  String? _error;
  ProviderSubscription<WalletMeta?>? _walletSubscription;

  @override
  void initState() {
    super.initState();
    ref.listen<WalletMeta?>(
      activeWalletMetaProvider,
      (previous, next) {
        if (next != null && _exactHeightController.text.isEmpty) {
          _exactHeightController.text = next.birthdayHeight.toString();
        }
      },
    );
    // Fire immediately manually
    final currentWallet = ref.read(activeWalletMetaProvider);
    if (currentWallet != null && _exactHeightController.text.isEmpty) {
      _exactHeightController.text = currentWallet.birthdayHeight.toString();
    }
    _loadLatestHeight();
  }

  @override
  void dispose() {
    _walletSubscription?.close();
    _exactHeightController.dispose();
    super.dispose();
  }

  List<int> get _yearOptions {
    final nowYear = DateTime.now().year;
    final count = (nowYear - _minYear).clamp(0, 200);
    return List<int>.generate(count + 1, (i) => nowYear - i);
  }

  int? get _selectedHeight {
    if (_inputMode == BirthdayHeightInputMode.exactHeight) {
      return int.tryParse(_exactHeightController.text.trim());
    }
    return _heightFromDate();
  }

  int? _heightFromDate() {
    if (_latestHeight == null) return null;
    final now = DateTime.now();
    final selected = DateTime(_selectedYear, _selectedMonth, 1);
    final daysAgo = now.difference(selected).inDays;
    final safeDays = daysAgo < 0 ? 0 : daysAgo;
    final offset = safeDays * _blocksPerDay;
    return (_latestHeight! - offset).clamp(1, _latestHeight!);
  }

  Future<void> _loadLatestHeight() async {
    setState(() {
      _loadingHeight = true;
      _heightError = null;
    });

    try {
      final config = await ref.read(lightdEndpointConfigProvider.future);
      final result = await FfiBridge.testNode(
        url: config.url,
        tlsPin: config.tlsPin,
      );
      if (!mounted) return;
      if (result.success && result.latestBlockHeight != null) {
        setState(() => _latestHeight = result.latestBlockHeight);
      } else {
        setState(() {
          _heightError =
              result.errorMessage ?? 'Unable to fetch latest block height.';
        });
      }
    } catch (_) {
      if (!mounted) return;
      setState(() => _heightError = 'Unable to fetch latest block height.');
    } finally {
      if (mounted) {
        setState(() => _loadingHeight = false);
      }
    }
  }

  Future<void> _saveBirthdayHeight() async {
    if (_isSaving) return;

    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      setState(() => _error = 'No active wallet selected.');
      return;
    }

    final selectedHeight = _selectedHeight;
    if (_inputMode == BirthdayHeightInputMode.approxDate && _latestHeight == null) {
      setState(() => _error = 'Load the latest block height or enter one manually.');
      return;
    }
    if (selectedHeight == null || selectedHeight <= 0) {
      setState(() => _error = 'Enter a valid block height.');
      return;
    }
    if (_latestHeight != null && selectedHeight > _latestHeight!) {
      setState(() => _error = 'Block height cannot be higher than the network tip.');
      return;
    }

    setState(() {
      _isSaving = true;
      _error = null;
    });

    try {
      await FfiBridge.setWalletBirthdayHeight(walletId, selectedHeight);
      ref.read(refreshWalletsProvider)();

      final rescan = await _confirmRescan(context);
      if (mounted) {
        setState(() => _isSaving = false);
      }

      if (rescan == true) {
        // Kick off rescan without blocking the UI.
        () async {
          try {
            await FfiBridge.rescan(walletId, selectedHeight);
          } catch (e) {
            if (mounted) {
              setState(() => _error = 'Rescan failed to start.');
            }
          }
        }();
        // Invalidate sync progress so home screen updates immediately.
        ref.invalidate(syncProgressStreamProvider);
        ref.invalidate(syncStatusProvider);
      }

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              rescan == true
                  ? 'Rescanning from block ${_formatHeight(selectedHeight)}...'
                  : 'Birthday height saved.',
            ),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = 'Failed to update birthday height.');
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  Future<bool?> _confirmRescan(BuildContext context) {
    return showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.surfaceElevated,
        title: const Text('Rescan now?'),
        content: const Text(
          'Rescanning applies the new birthday height and rebuilds wallet state.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Later'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Rescan'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final meta = ref.watch(activeWalletMetaProvider);
    final selectedHeight = _selectedHeight;
    final tip = _latestHeight;
    final blocksToScan = (selectedHeight != null && tip != null)
        ? (tip - selectedHeight).clamp(0, tip)
        : null;
    final basePadding = AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );

    return PScaffold(
      title: 'Birthday Height',
      appBar: const PAppBar(
        title: 'Birthday Height',
        subtitle: 'Set the earliest block to scan',
        showBackButton: true,
      ),
      body: SingleChildScrollView(
        padding: contentPadding,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (meta != null) ...[
              Text(
                'Current: Block ${_formatHeight(meta.birthdayHeight)}',
                style: AppTypography.caption.copyWith(
                  color: AppColors.textSecondary,
                ),
              ),
              const SizedBox(height: AppSpacing.sm),
            ],
            PCard(
              child: Padding(
                padding: const EdgeInsets.all(AppSpacing.md),
                child: Row(
                  children: [
                    Icon(
                      Icons.public,
                      color: AppColors.accentPrimary,
                    ),
                    const SizedBox(width: AppSpacing.sm),
                    Expanded(
                      child: Text(
                        _loadingHeight
                            ? 'Fetching latest block height...'
                            : tip == null
                                ? 'Latest block height unavailable'
                                : 'Network tip: ${_formatHeight(tip)}',
                        style: AppTypography.body.copyWith(
                          color: AppColors.textPrimary,
                        ),
                      ),
                    ),
                    PTextButton(
                      label: _loadingHeight ? 'Loading' : 'Refresh',
                      onPressed: _loadingHeight ? null : _loadLatestHeight,
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
                  color: AppColors.error,
                ),
              ),
            ],
            const SizedBox(height: AppSpacing.lg),
            _ModeCard(
              mode: BirthdayHeightInputMode.exactHeight,
              selected: _inputMode,
              title: 'Exact block height',
              subtitle: 'Use the precise block height if you know it',
              icon: Icons.pin_outlined,
              onTap: () => setState(() => _inputMode = BirthdayHeightInputMode.exactHeight),
            ),
            const SizedBox(height: AppSpacing.sm),
            _ModeCard(
              mode: BirthdayHeightInputMode.approxDate,
              selected: _inputMode,
              title: 'Approximate date',
              subtitle: 'Month and year before your first transaction',
              icon: Icons.calendar_today,
              onTap: () => setState(() => _inputMode = BirthdayHeightInputMode.approxDate),
            ),
            const SizedBox(height: AppSpacing.lg),
            if (_inputMode == BirthdayHeightInputMode.exactHeight) ...[
              PInput(
                controller: _exactHeightController,
                label: 'Block height',
                hint: 'Enter the exact block height',
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly],
              ),
            ] else ...[
              Row(
                children: [
                  Expanded(
                    child: DropdownButtonFormField<int>(
                      key: ValueKey(_selectedMonth),
                      initialValue: _selectedMonth,
                      decoration: InputDecoration(
                        labelText: 'Month',
                        filled: true,
                        fillColor: AppColors.surfaceElevated,
                      ),
                      items: List.generate(
                        _monthLabels.length,
                        (index) => DropdownMenuItem(
                          value: index + 1,
                          child: Text(_monthLabels[index]),
                        ),
                      ),
                      onChanged: (value) {
                        if (value == null) return;
                        setState(() => _selectedMonth = value);
                      },
                    ),
                  ),
                  const SizedBox(width: AppSpacing.md),
                  Expanded(
                    child: DropdownButtonFormField<int>(
                      key: ValueKey(_selectedYear),
                      initialValue: _selectedYear,
                      decoration: InputDecoration(
                        labelText: 'Year',
                        filled: true,
                        fillColor: AppColors.surfaceElevated,
                      ),
                      items: _yearOptions
                          .map(
                            (year) => DropdownMenuItem(
                              value: year,
                              child: Text(year.toString()),
                            ),
                          )
                          .toList(),
                      onChanged: (value) {
                        if (value == null) return;
                        setState(() => _selectedYear = value);
                      },
                    ),
                  ),
                ],
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
                          'Start block',
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
                      crossAxisAlignment:
                          isNarrow ? CrossAxisAlignment.start : CrossAxisAlignment.end,
                      children: [
                        Text(
                          'Blocks to scan',
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
                      children: [
                        startBlock,
                        blocksScan,
                      ],
                    );
                  },
                ),
              ),
            ),
            if (_error != null) ...[
              const SizedBox(height: AppSpacing.md),
              Container(
                padding: const EdgeInsets.all(AppSpacing.md),
                decoration: BoxDecoration(
                  color: AppColors.error.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(
                    color: AppColors.error.withValues(alpha: 0.3),
                  ),
                ),
                child: Row(
                  children: [
                    Icon(Icons.error_outline, color: AppColors.error, size: 18),
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
            ],
            const SizedBox(height: AppSpacing.xl),
            PButton(
              text: _isSaving ? 'Saving...' : 'Save birthday height',
              onPressed: _isSaving ? null : _saveBirthdayHeight,
              variant: PButtonVariant.primary,
              size: PButtonSize.large,
              isLoading: _isSaving,
            ),
          ],
        ),
      ),
    );
  }

  String _formatHeight(int height) {
    return height.toString().replaceAllMapped(
      RegExp(r'(\\d{1,3})(?=(\\d{3})+(?!\\d))'),
      (Match m) => '${m[1]},',
    );
  }
}

class _ModeCard extends StatelessWidget {
  final BirthdayHeightInputMode mode;
  final BirthdayHeightInputMode selected;
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
                ? Border.all(
                    color: AppColors.accentPrimary,
                    width: 2,
                  )
                : null,
            borderRadius: BorderRadius.circular(16),
          ),
          child: Row(
            children: [
              Icon(
                icon,
                color: isSelected ? AppColors.accentPrimary : AppColors.textSecondary,
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
              RadioGroup<BirthdayHeightInputMode>(
                groupValue: selected,
                onChanged: (_) => onTap(),
                child: Radio<BirthdayHeightInputMode>(
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
