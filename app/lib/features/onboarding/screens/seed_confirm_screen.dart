// Seed Confirm Screen - Verify user has backed up their seed phrase

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../core/crypto/bip39_wordlist.dart';
import '../../../core/security/screenshot_protection.dart';
import '../../../design/deep_space_theme.dart';
import '../../../design/tokens/spacing.dart';
import '../../../ui/atoms/p_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';
import '../onboarding_flow.dart';
import '../../../core/providers/wallet_providers.dart';
import '../widgets/onboarding_progress_indicator.dart';

class SeedConfirmScreen extends ConsumerStatefulWidget {
  const SeedConfirmScreen({super.key});

  @override
  ConsumerState<SeedConfirmScreen> createState() => _SeedConfirmScreenState();
}

class _SeedConfirmScreenState extends ConsumerState<SeedConfirmScreen> {
  final List<TextEditingController> _wordControllers =
      List.generate(3, (_) => TextEditingController());
  final List<FocusNode> _focusNodes = List.generate(3, (_) => FocusNode());
  List<int> _selectedIndices = [];
  bool _isVerifying = false;
  String? _error;
  ScreenProtection? _screenProtection;

  @override
  void initState() {
    super.initState();
    _disableScreenshots();
    _selectRandomWords();
    // Add listeners to update button state when text changes
    for (final controller in _wordControllers) {
      controller.addListener(_onTextChanged);
    }
  }

  @override
  void dispose() {
    for (final controller in _wordControllers) {
      controller
        ..removeListener(_onTextChanged)
        ..dispose();
    }
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

  void _selectRandomWords() {
    // Select 3 random word positions (1-24)
    final random = DateTime.now().millisecondsSinceEpoch;
    final randomGenerator = random % 1000000;
    _selectedIndices = [];
    final used = <int>{};
    
    while (_selectedIndices.length < 3) {
      final index = ((randomGenerator + _selectedIndices.length * 7) % 24) + 1;
      if (!used.contains(index)) {
        _selectedIndices.add(index);
        used.add(index);
      }
      if (used.length >= 24) break; // Safety check
    }
    _selectedIndices.sort();
    setState(() {});
  }

  bool get _isComplete {
    return _wordControllers.every((c) => c.text.trim().isNotEmpty);
  }

  Future<void> _verifyAndProceed() async {
    if (!_isComplete) return;

    setState(() {
      _isVerifying = true;
      _error = null;
    });

    try {
      final state = ref.read(onboardingControllerProvider);
      final mnemonic = state.mnemonic;
      
      if (mnemonic == null || mnemonic.isEmpty) {
        throw StateError('Mnemonic not found in onboarding state');
      }

      final words = mnemonic.split(' ');
      
      // Verify each word
      for (int i = 0; i < _selectedIndices.length; i++) {
        final index = _selectedIndices[i] - 1; // Convert to 0-based
        final expectedWord = words[index].toLowerCase().trim();
        final enteredWord = _wordControllers[i].text.toLowerCase().trim();
        
        if (expectedWord != enteredWord) {
          setState(() {
            _error = 'Word ${_selectedIndices[i]} is incorrect. Please check and try again.';
            _isVerifying = false;
          });
          return;
        }
      }

      // Verification successful
      ref.read(onboardingControllerProvider.notifier).markSeedBackedUp();
      ref.read(onboardingControllerProvider.notifier).nextStep();
      
      // Create wallet with the mnemonic we generated
      await ref.read(onboardingControllerProvider.notifier).complete('My Pirate Wallet');
      
      if (mounted) {
        // Invalidate walletsExistProvider to refresh it after wallet creation
        ref.invalidate(walletsExistProvider);
        // Ensure app is marked as unlocked
        ref.read(appUnlockedProvider.notifier).unlocked = true;
        // Navigate to home
        context.go('/home');
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Wallet created. Syncing...'),
            backgroundColor: AppColors.success,
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    } catch (e) {
      setState(() {
        _error = 'Verification failed: $e';
        _isVerifying = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final basePadding = AppSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: AppSpacing.xl,
    );
    final contentPadding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return PScaffold(
      title: 'Confirm Seed',
      appBar: const PAppBar(
        title: 'Verify Your Backup',
        subtitle: 'Confirm you wrote it down',
        showBackButton: true,
      ),
      body: CustomScrollView(
        keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
        slivers: [
          SliverPadding(
            padding: contentPadding,
            sliver: SliverFillRemaining(
              hasScrollBody: false,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const OnboardingProgressIndicator(
                    currentStep: 6,
                    totalSteps: 6,
                  ),
                  const SizedBox(height: AppSpacing.xxl),
                  Text(
                    'Enter these words from your seed phrase',
                    style: AppTypography.h2.copyWith(
                      color: AppColors.textPrimary,
                    ),
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: AppSpacing.sm),
                  Text(
                    "This confirms you've written down your seed phrase correctly.",
                    style: AppTypography.body.copyWith(
                      color: AppColors.textSecondary,
                    ),
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: AppSpacing.xl),

                  // Word inputs
                  ...List.generate(3, (i) {
                    return Padding(
                      padding: const EdgeInsets.only(bottom: AppSpacing.md),
                      child: _SeedWordInput(
                        controller: _wordControllers[i],
                        focusNode: _focusNodes[i],
                        label: 'Word ${_selectedIndices[i]}',
                        hint: 'Enter word ${_selectedIndices[i]}',
                        textInputAction:
                            i < 2 ? TextInputAction.next : TextInputAction.done,
                        autofocus: i == 0,
                        onSubmitted: () {
                          if (i < 2) {
                            _focusNodes[i + 1].requestFocus();
                          } else {
                            _verifyAndProceed();
                          }
                        },
                      ),
                    );
                  }),

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
                  ],

                  const Spacer(),

                  PButton(
                    text: 'Verify & Create Wallet',
                    onPressed:
                        _isComplete && !_isVerifying ? _verifyAndProceed : null,
                    variant: PButtonVariant.primary,
                    size: PButtonSize.large,
                    isLoading: _isVerifying,
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SeedWordInput extends StatefulWidget {
  final TextEditingController controller;
  final FocusNode focusNode;
  final String label;
  final String hint;
  final TextInputAction textInputAction;
  final bool autofocus;
  final VoidCallback onSubmitted;

  const _SeedWordInput({
    required this.controller,
    required this.focusNode,
    required this.label,
    required this.hint,
    required this.textInputAction,
    required this.autofocus,
    required this.onSubmitted,
  });

  @override
  State<_SeedWordInput> createState() => _SeedWordInputState();
}

class _SeedWordInputState extends State<_SeedWordInput> {
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
  void didUpdateWidget(covariant _SeedWordInput oldWidget) {
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
    const itemHeight = 44.0;
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
                            fontSize: 16,
                          ),
                          children: [
                            TextSpan(
                              text: typed,
                              style: AppTypography.body.copyWith(
                                color: AppColors.accentPrimary,
                                fontSize: 16,
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
    if (widget.textInputAction == TextInputAction.done) {
      widget.focusNode.unfocus();
      widget.onSubmitted();
    } else {
      FocusScope.of(context).nextFocus();
    }
  }

  void _applyUniqueCompletion() {
    final current = widget.controller.text.trim().toLowerCase();
    final matches =
        current.isEmpty ? const <String>[] : bip39Suggestions(current, limit: 2);
    if (matches.length == 1 && matches.first != current) {
      widget.controller.text = matches.first;
      widget.controller.selection = TextSelection.fromPosition(
        TextPosition(offset: matches.first.length),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final inputStyle =
        AppTypography.body.copyWith(color: AppColors.textPrimary);
    final contentPadding =
        Theme.of(context).inputDecorationTheme.contentPadding ??
            const EdgeInsets.symmetric(horizontal: 16, vertical: 14);
    final reduceMotion = MediaQuery.of(context).disableAnimations;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          widget.label,
          style: AppTypography.labelMedium.copyWith(
            color: AppColors.textSecondary,
          ),
        ),
        const SizedBox(height: AppSpacing.xs),
        AnimatedContainer(
          duration:
              reduceMotion ? Duration.zero : const Duration(milliseconds: 200),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(PSpacing.radiusInput),
            boxShadow: _isFocused
                ? [
                    BoxShadow(
                      color: AppColors.focusRingSubtle,
                      blurRadius: 8.0,
                      offset: Offset.zero,
                    ),
                  ]
                : null,
          ),
          child: Stack(
            alignment: Alignment.centerLeft,
            children: [
              CompositedTransformTarget(
                link: _layerLink,
                child: SizedBox(
                  key: _fieldKey,
                  child: TextField(
                    controller: widget.controller,
                    focusNode: widget.focusNode,
                    textInputAction: widget.textInputAction,
                    onSubmitted: (_) {
                      _applyUniqueCompletion();
                      widget.onSubmitted();
                    },
                    autofocus: widget.autofocus,
                    autocorrect: false,
                    enableSuggestions: false,
                    inputFormatters: [
                      FilteringTextInputFormatter.allow(RegExp('[a-zA-Z]')),
                      const _LowerCaseTextFormatter(),
                    ],
                    style: inputStyle,
                    decoration: InputDecoration(
                      hintText: widget.hint,
                      filled: true,
                      fillColor: AppColors.backgroundSurface,
                      contentPadding: contentPadding,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ],
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
