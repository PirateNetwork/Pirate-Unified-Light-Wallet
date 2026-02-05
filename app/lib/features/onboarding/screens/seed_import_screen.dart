// Seed Import Screen - Restore wallet from 24-word mnemonic

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../../../core/crypto/bip39_wordlist.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/security/screenshot_protection.dart';
import '../onboarding_flow.dart';
import '../widgets/onboarding_progress_indicator.dart';

/// Seed import screen for wallet restoration
class SeedImportScreen extends ConsumerStatefulWidget {
  const SeedImportScreen({super.key});

  @override
  ConsumerState<SeedImportScreen> createState() => _SeedImportScreenState();
}

class _SeedImportScreenState extends ConsumerState<SeedImportScreen> {
  final _formKey = GlobalKey<FormState>();
  final List<TextEditingController> _wordControllers =
      List.generate(24, (_) => TextEditingController());
  final List<FocusNode> _focusNodes =
      List.generate(24, (_) => FocusNode());
  ScreenProtection? _screenProtection;

  bool _isValidating = false;
  String? _validationError;
  int _wordCount = 24;
  bool _isPasting = false;

  @override
  void initState() {
    super.initState();
    _disableScreenshots();
    // Add listeners to update button state when text changes
    for (final controller in _wordControllers) {
      controller.addListener(_onTextChanged);
    }
    _wordControllers.first.addListener(_onFirstWordChanged);
  }

  @override
  void dispose() {
    for (final controller in _wordControllers) {
      controller
        ..removeListener(_onTextChanged)
        ..dispose();
    }
    _wordControllers.first.removeListener(_onFirstWordChanged);
    for (final node in _focusNodes) {
      node.dispose();
    }
    _enableScreenshots();
    super.dispose();
  }

  void _disableScreenshots() {
    if (_screenProtection != null) return;
    _screenProtection = ScreenshotProtection.protect();
  }

  void _enableScreenshots() {
    _screenProtection?.dispose();
    _screenProtection = null;
  }

  void _onTextChanged() {
    setState(() {}); // Rebuild to update button state
  }

  void _onFirstWordChanged() {
    if (_isPasting) return;
    final raw = _wordControllers.first.text;
    if (!raw.contains(RegExp(r'\s'))) return;
    final words = raw.trim().split(RegExp(r'\s+')).where((w) => w.isNotEmpty);
    if (words.length <= 1) return;
    _applyPastedWords(words.toList());
  }

  void _applyPastedWords(List<String> words) {
    if (words.isEmpty) return;
    _isPasting = true;
    try {
      final normalized = words.map((word) => word.toLowerCase()).toList();
      final nextCount = normalized.length <= 12 ? 12 : 24;
      if (_wordCount != nextCount) {
        setState(() => _wordCount = nextCount);
      }
      for (int i = 0; i < _wordControllers.length; i++) {
        if (i < nextCount && i < normalized.length) {
          _wordControllers[i].text = normalized[i];
        } else {
          _wordControllers[i].clear();
        }
      }
    } finally {
      _isPasting = false;
    }
  }

  String get _mnemonic {
    return _wordControllers
        .take(_wordCount)
        .map((c) => c.text.trim().toLowerCase())
        .join(' ');
  }

  bool get _isComplete {
    return _wordControllers
        .take(_wordCount)
        .every((c) => c.text.trim().isNotEmpty);
  }

  Future<void> _validateAndProceed() async {
    if (!_isComplete) return;

    setState(() {
      _isValidating = true;
      _validationError = null;
    });

    try {
      // Validate mnemonic via FFI
      final isValid = await FfiBridge.validateMnemonic(_mnemonic);
      
      if (!isValid) {
        setState(() {
          _validationError = 'Invalid seed phrase. Check the words and order.';
          _isValidating = false;
        });
        return;
      }

      // Store mnemonic in onboarding state
      ref.read(onboardingControllerProvider.notifier).setMnemonic(_mnemonic);

      final hasPassphrase = await FfiBridge.hasAppPassphrase();
      if (!hasPassphrase) {
        if (mounted) {
          unawaited(context.push('/onboarding/passphrase'));
        }
        return;
      }

      // Skip passphrase setup if one already exists.
      ref.read(onboardingControllerProvider.notifier).nextStep();
      ref.read(onboardingControllerProvider.notifier).nextStep();

      if (mounted) {
      unawaited(context.push('/onboarding/birthday'));
      }
    } catch (e) {
      setState(() {
        _validationError = 'Could not validate the phrase. Try again.';
        _isValidating = false;
      });
    }
  }

  Future<void> _pasteFromClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    if (data?.text == null) return;

    final words = data!.text!.trim().split(RegExp(r'\s+'));
    _applyPastedWords(words);
  }

  void _clearAll() {
    for (final controller in _wordControllers) {
      controller.clear();
    }
    setState(() {
      _validationError = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final gutter = AppSpacing.responsiveGutter(MediaQuery.of(context).size.width);
    final viewInsets = MediaQuery.of(context).viewInsets.bottom;
    return PScaffold(
      title: 'Import Seed',
      appBar: PAppBar(
        title: 'Import Seed Phrase',
        subtitle: 'Restore a wallet with your seed phrase',
        onBack: () => context.pop(),
        centerTitle: true,
        actions: [
          PIconButton(
            icon: const Icon(Icons.content_paste),
            tooltip: 'Paste from clipboard',
            onPressed: _pasteFromClipboard,
          ),
        ],
      ),
      body: Column(
        children: [
          // Progress indicator
          Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              AppSpacing.lg,
              gutter,
              AppSpacing.lg,
            ),
            child: const OnboardingProgressIndicator(
              currentStep: 2,
              totalSteps: 5,
            ),
          ),

          Expanded(
            child: CustomScrollView(
              keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
              slivers: [
                SliverPadding(
                  padding: EdgeInsets.fromLTRB(
                    gutter,
                    0,
                    gutter,
                    AppSpacing.lg + viewInsets,
                  ),
                  sliver: SliverToBoxAdapter(
                    child: Form(
                      key: _formKey,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          // Title
                          Text(
                            'Enter your seed phrase',
                            style: AppTypography.h2.copyWith(
                              color: AppColors.textPrimary,
                            ),
                          ),

                          const SizedBox(height: AppSpacing.sm),

                          Text(
                            'Enter the $_wordCount words in order.',
                            style: AppTypography.body.copyWith(
                              color: AppColors.textSecondary,
                            ),
                          ),

                          const SizedBox(height: AppSpacing.md),

                          // Word count toggle
                          Row(
                            children: [
                              _WordCountChip(
                                label: '12 words',
                                selected: _wordCount == 12,
                                onTap: () => setState(() => _wordCount = 12),
                              ),
                              const SizedBox(width: AppSpacing.sm),
                              _WordCountChip(
                                label: '24 words',
                                selected: _wordCount == 24,
                                onTap: () => setState(() => _wordCount = 24),
                              ),
                              const Spacer(),
                              PTextButton(
                                label: 'Clear all',
                                leadingIcon: Icons.clear,
                                variant: PTextButtonVariant.subtle,
                                onPressed: _clearAll,
                              ),
                            ],
                          ),

                          const SizedBox(height: AppSpacing.lg),

                          // Word grid
                          GridView.builder(
                            shrinkWrap: true,
                            physics: const NeverScrollableScrollPhysics(),
                            gridDelegate:
                                const SliverGridDelegateWithFixedCrossAxisCount(
                              crossAxisCount: 3,
                              childAspectRatio: 2.5,
                              crossAxisSpacing: AppSpacing.sm,
                              mainAxisSpacing: AppSpacing.sm,
                            ),
                            itemCount: _wordCount,
                            itemBuilder: (context, index) {
                              return _WordInput(
                                index: index,
                                controller: _wordControllers[index],
                                focusNode: _focusNodes[index],
                                isLast: index == _wordCount - 1,
                                onSubmitted: () {
                                  if (index < _wordCount - 1) {
                                    _focusNodes[index + 1].requestFocus();
                                  } else {
                                    _focusNodes[index].unfocus();
                                    _validateAndProceed();
                                  }
                                },
                              );
                            },
                          ),

                          const SizedBox(height: AppSpacing.lg),

                          // Error message
                          if (_validationError != null)
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
                                  Icon(
                                    Icons.error_outline,
                                    color: AppColors.error,
                                    size: 20,
                                  ),
                                  const SizedBox(width: AppSpacing.sm),
                                  Expanded(
                                    child: Text(
                                      _validationError!,
                                      style: AppTypography.body.copyWith(
                                        color: AppColors.error,
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            ),

                          const SizedBox(height: AppSpacing.lg),

                          // Security notice
                          Container(
                            padding: const EdgeInsets.all(AppSpacing.md),
                            decoration: BoxDecoration(
                              color: AppColors.warning.withValues(alpha: 0.1),
                              borderRadius: BorderRadius.circular(12),
                              border: Border.all(
                                color: AppColors.warning.withValues(alpha: 0.3),
                              ),
                            ),
                            child: Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Icon(
                                  Icons.security,
                                  color: AppColors.warning,
                                  size: 20,
                                ),
                                const SizedBox(width: AppSpacing.sm),
                                Expanded(
                                  child: Text(
                                    'Keep this private. Anyone with your seed phrase '
                                    'can access your funds.',
                                    style: AppTypography.caption.copyWith(
                                      color: AppColors.textPrimary,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ),

                          PButton(
                            text:
                                _isValidating ? 'Validating...' : 'Continue',
                            onPressed: _isComplete && !_isValidating
                                ? _validateAndProceed
                            : null,
                            variant: PButtonVariant.primary,
                            size: PButtonSize.large,
                            isLoading: _isValidating,
                          ),
                          const SizedBox(height: AppSpacing.sm),
                        ],
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Word input field
class _WordInput extends StatefulWidget {
  final int index;
  final TextEditingController controller;
  final FocusNode focusNode;
  final bool isLast;
  final VoidCallback onSubmitted;

  const _WordInput({
    required this.index,
    required this.controller,
    required this.focusNode,
    required this.isLast,
    required this.onSubmitted,
  });

  @override
  State<_WordInput> createState() => _WordInputState();
}

class _WordInputState extends State<_WordInput> {
  final LayerLink _layerLink = LayerLink();
  final GlobalKey _fieldKey = GlobalKey();
  OverlayEntry? _overlay;
  List<String> _matches = const [];
  bool _isFocused = false;

  @override
  void initState() {
    super.initState();
    widget.focusNode.addListener(_onFocusChange);
    widget.controller.addListener(_onTextChanged);
  }

  @override
  void didUpdateWidget(covariant _WordInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.focusNode != widget.focusNode) {
      oldWidget.focusNode.removeListener(_onFocusChange);
      widget.focusNode.addListener(_onFocusChange);
    }
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onTextChanged);
      widget.controller.addListener(_onTextChanged);
    }
  }

  @override
  void dispose() {
    widget.focusNode.removeListener(_onFocusChange);
    widget.controller.removeListener(_onTextChanged);
    _removeOverlay();
    super.dispose();
  }

  void _onFocusChange() {
    if (!mounted) return;
    _isFocused = widget.focusNode.hasFocus;
    if (_isFocused) {
      _refreshOverlay();
    } else {
      Future.delayed(const Duration(milliseconds: 120), () {
        if (!mounted) return;
        if (!widget.focusNode.hasFocus) {
          _removeOverlay();
        }
      });
    }
    setState(() {});
  }

  void _onTextChanged() {
    _updateMatches();
  }

  void _updateMatches() {
    final query = widget.controller.text.trim().toLowerCase();
    final nextMatches =
        query.isEmpty ? const <String>[] : bip39Suggestions(query, limit: 6);
    if (!_listEquals(_matches, nextMatches)) {
      _matches = nextMatches;
    }
    _refreshOverlay();
    if (mounted) {
      setState(() {});
    }
  }

  bool _listEquals(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  void _refreshOverlay() {
    final query = widget.controller.text.trim().toLowerCase();
    final exactMatch = _matches.length == 1 && _matches.first == query;
    final shouldShow = _isFocused && _matches.isNotEmpty && !exactMatch;
    if (shouldShow) {
      _showOverlay();
    } else {
      _removeOverlay();
    }
  }

  void _showOverlay() {
    if (_overlay == null) {
      _overlay = OverlayEntry(builder: (context) {
        return Stack(
          children: [
            // Invisible barrier to catch taps outside
            Positioned.fill(
              child: GestureDetector(
                onTap: _removeOverlay,
                behavior: HitTestBehavior.translucent,
                child: Container(color: Colors.transparent),
              ),
            ),
            _buildOverlay(context),
          ],
        );
      });
      Overlay.of(context, rootOverlay: false).insert(_overlay!);
    } else {
      _overlay?.markNeedsBuild();
    }
  }

  void _removeOverlay() {
    _overlay?.remove();
    _overlay = null;
  }

  Widget _buildOverlay(BuildContext context) {
    final renderBox = _fieldKey.currentContext?.findRenderObject() as RenderBox?;
    if (renderBox == null) return const SizedBox.shrink();

    final fieldSize = renderBox.size;
    final width = fieldSize.width;
    final height = fieldSize.height;
    const itemHeight = 40.0;
    final listHeight = (_matches.length * itemHeight + 8).clamp(itemHeight, itemHeight * 4.5 + 8);
    final typed = widget.controller.text.trim().toLowerCase();
    
    final screenHeight = MediaQuery.of(context).size.height;
    final fieldOffset = renderBox.localToGlobal(Offset.zero);
    final spaceBelow = screenHeight - fieldOffset.dy - height;
    final showAbove = spaceBelow < (listHeight + 20);
    final offsetY = showAbove ? -(listHeight + 4) : height + 4;

    return CompositedTransformFollower(
      link: _layerLink,
      showWhenUnlinked: false,
      offset: Offset(0, offsetY),
      child: Material(
        color: Colors.transparent,
        child: Container(
          width: width,
          decoration: BoxDecoration(
            color: AppColors.surfaceElevated,
            borderRadius: BorderRadius.circular(12),
            border: Border.all(color: AppColors.accentPrimary.withValues(alpha: 0.5), width: 1.5),
            boxShadow: [
              BoxShadow(
                color: Colors.black.withValues(alpha: 0.4),
                blurRadius: 16,
                spreadRadius: 2,
                offset: const Offset(0, 8),
              ),
            ],
          ),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(10),
            child: ConstrainedBox(
              constraints: BoxConstraints(maxHeight: listHeight),
              child: ListView.builder(
                padding: const EdgeInsets.symmetric(vertical: 4),
                shrinkWrap: true,
                physics: const ClampingScrollPhysics(),
                itemCount: _matches.length,
                itemBuilder: (context, index) {
                  final word = _matches[index];
                  return InkWell(
                    onTap: () => _selectSuggestion(word),
                    child: Container(
                      height: itemHeight,
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      alignment: Alignment.centerLeft,
                      decoration: BoxDecoration(
                        border: index < _matches.length - 1 
                          ? Border(bottom: BorderSide(color: AppColors.border.withValues(alpha: 0.3)))
                          : null,
                      ),
                      child: RichText(
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        text: TextSpan(
                          style: AppTypography.body.copyWith(
                            color: AppColors.textSecondary,
                            fontSize: 15,
                          ),
                          children: [
                            TextSpan(
                              text: typed,
                              style: AppTypography.body.copyWith(
                                color: AppColors.accentPrimary,
                                fontSize: 15,
                                fontWeight: FontWeight.w700,
                              ),
                            ),
                            TextSpan(text: word.substring(typed.length)),
                          ],
                        ),
                      ),
                    ),
                  );
                },
              ),
            ),
          ),
        ),
      ),
    );
  }

  void _selectSuggestion(String word) {
    widget.controller.text = word;
    widget.controller.selection = TextSelection.fromPosition(
      TextPosition(offset: word.length),
    );
    _removeOverlay();
    if (widget.isLast) {
      widget.focusNode.unfocus();
      widget.onSubmitted();
    } else {
      FocusScope.of(context).nextFocus();
    }
  }

  void _applyUniqueCompletion() {
    final current = widget.controller.text.trim().toLowerCase();
    final completionMatches =
        current.isEmpty ? const <String>[] : bip39Suggestions(current, limit: 2);
    if (completionMatches.length == 1 && completionMatches.first != current) {
      widget.controller.text = completionMatches.first;
      widget.controller.selection = TextSelection.fromPosition(
        TextPosition(offset: completionMatches.first.length),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final typed = widget.controller.text.trim().toLowerCase();
    final matches =
        typed.isEmpty ? const <String>[] : bip39Suggestions(typed, limit: 1);
    final hasPrefixMatch = typed.isEmpty || matches.isNotEmpty;
    final borderColor = hasPrefixMatch
        ? (_isFocused ? AppColors.accentPrimary : AppColors.border)
        : AppColors.error;
    final inputStyle = AppTypography.body.copyWith(
      color: AppColors.textPrimary,
      fontSize: 14,
    );
    final reduceMotion = MediaQuery.of(context).disableAnimations;

    return AnimatedContainer(
      duration:
          reduceMotion ? Duration.zero : const Duration(milliseconds: 160),
      decoration: BoxDecoration(
        color: AppColors.surfaceElevated,
        borderRadius: BorderRadius.circular(PSpacing.radiusInput),
        border: Border.all(color: borderColor),
        boxShadow: _isFocused
            ? [
                BoxShadow(
                  color: AppColors.focusRingSubtle,
                  blurRadius: 8,
                  offset: Offset.zero,
                ),
              ]
            : null,
      ),
      child: Row(
        children: [
          // Word number
          Container(
            width: 28,
            alignment: Alignment.center,
            child: Text(
              '${widget.index + 1}',
              style: AppTypography.caption.copyWith(
                color: AppColors.textTertiary,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          // Word input
          Expanded(
            child: CompositedTransformTarget(
              link: _layerLink,
              child: SizedBox(
                key: _fieldKey,
                child: TextField(
                  controller: widget.controller,
                  focusNode: widget.focusNode,
                  style: inputStyle,
                  decoration: const InputDecoration(
                    border: InputBorder.none,
                    contentPadding: EdgeInsets.symmetric(
                      horizontal: 4,
                      vertical: 8,
                    ),
                    isDense: true,
                  ),
                  autocorrect: false,
                  enableSuggestions: false,
                  textInputAction: widget.isLast
                      ? TextInputAction.done
                      : TextInputAction.next,
                  inputFormatters: [
                    FilteringTextInputFormatter.allow(RegExp('[a-zA-Z]')),
                    const _LowerCaseTextFormatter(),
                  ],
                  onSubmitted: (_) {
                    _applyUniqueCompletion();
                    widget.onSubmitted();
                  },
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _LowerCaseTextFormatter extends TextInputFormatter {
  const _LowerCaseTextFormatter();

  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    final lower = newValue.text.toLowerCase();
    return newValue.copyWith(
      text: lower,
      selection: newValue.selection,
      composing: TextRange.empty,
    );
  }
}

/// Word count selection chip
class _WordCountChip extends StatelessWidget {
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _WordCountChip({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    // Use smaller padding on desktop to match mobile size
    final isDesktop = MediaQuery.of(context).size.width > 600;
    final horizontalPadding = isDesktop ? AppSpacing.sm : AppSpacing.md;
    final verticalPadding = isDesktop ? AppSpacing.xs : AppSpacing.sm;
    
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: EdgeInsets.symmetric(
          horizontal: horizontalPadding,
          vertical: verticalPadding,
        ),
        decoration: BoxDecoration(
          color: selected 
              ? AppColors.accentPrimary.withValues(alpha: 0.1) 
              : AppColors.surfaceElevated,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: selected 
                ? AppColors.accentPrimary 
                : AppColors.border,
          ),
        ),
        child: Text(
          label,
          style: AppTypography.caption.copyWith(
            color: selected 
                ? AppColors.accentPrimary 
                : AppColors.textSecondary,
            fontWeight: selected ? FontWeight.w600 : FontWeight.normal,
          ),
        ),
      ),
    );
  }
}
