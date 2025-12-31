import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_auth/local_auth.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../design/theme.dart';
import '../../design/compat.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../core/ffi/ffi_bridge.dart';
import 'dart:async';

/// Provider for clipboard countdown timer
class ClipboardCountdownNotifier extends Notifier<int?> {
  @override
  int? build() => null;
}

final _clipboardCountdownProvider = NotifierProvider<ClipboardCountdownNotifier, int?>(ClipboardCountdownNotifier.new);

/// Export Seed Screen with security gating
class ExportSeedScreen extends ConsumerStatefulWidget {
  final String walletId;
  final String walletName;

  const ExportSeedScreen({
    Key? key,
    required this.walletId,
    required this.walletName,
  }) : super(key: key);

  @override
  ConsumerState<ExportSeedScreen> createState() => _ExportSeedScreenState();
}

class _ExportSeedScreenState extends ConsumerState<ExportSeedScreen> {
  final _passphraseController = TextEditingController();
  final _localAuth = LocalAuthentication();
  
  bool _step1Complete = false; // Warning acknowledged
  bool _step2Complete = false; // Biometric passed
  bool _step3Complete = false; // Passphrase verified
  bool _seedRevealed = false;
  bool _exportStarted = false;
  String? _mnemonic;
  bool _isLoading = false;
  String? _error;
  Timer? _clipboardTimer;

  @override
  void dispose() {
    _passphraseController.dispose();
    _clipboardTimer?.cancel();
    if (_exportStarted) {
      FfiBridge.cancelSeedExport();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: !_seedRevealed,
      onPopInvokedWithResult: (didPop, result) {
        if (_seedRevealed && !didPop) {
          _showExitConfirmation();
        }
      },
      child: Scaffold(
        backgroundColor: Colors.black,
        body: SafeArea(
          child: _buildContent(),
        ),
      ),
    );
  }

  Widget _buildContent() {
    if (!_step1Complete) {
      return _buildWarningStep();
    } else if (!_step2Complete) {
      return _buildBiometricStep();
    } else if (!_step3Complete) {
      return _buildPassphraseStep();
    } else {
      return _buildSeedDisplay();
    }
  }

  Widget _centeredStep(Widget child, {bool allowScroll = true}) {
    const maxWidth = 560.0;
    return LayoutBuilder(builder: (context, constraints) {
      final padding = PirateSpacing.xxl;
      final minHeight = (constraints.maxHeight - (padding * 2))
          .clamp(0.0, double.infinity);
      final centeredChild = ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: maxWidth,
          minHeight: minHeight,
        ),
        child: child,
      );
      if (!allowScroll) {
        return Center(child: centeredChild);
      }
      return SingleChildScrollView(
        padding: EdgeInsets.all(padding),
        child: Center(child: centeredChild),
      );
    });
  }

  /// Step 1: Full-screen warning
  Widget _buildWarningStep() {
    return _centeredStep(
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Spacer(),
          Icon(
            Icons.warning_rounded,
            size: 80,
            color: Colors.red[400],
          ),
          SizedBox(height: PirateSpacing.xl),
          Text(
            'Reveal recovery phrase',
            style: PirateTypography.h2.copyWith(color: Colors.white),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.xl),
          _buildWarningCard(
            icon: Icons.security,
            title: 'Never share your phrase',
            description:
                'Anyone with this phrase can spend your funds. Never share it.',
          ),
          SizedBox(height: PirateSpacing.lg),
          _buildWarningCard(
            icon: Icons.photo_camera,
            title: 'Store offline',
            description:
                'Write it down and store it offline. Avoid screenshots or digital copies.',
          ),
          SizedBox(height: PirateSpacing.lg),
          _buildWarningCard(
            icon: Icons.verified_user,
            title: 'We will never ask',
            description:
                'Support will never ask for your recovery phrase. Anyone asking is a scam.',
          ),
          const Spacer(),
          PButton(
            variant: PButtonVariant.danger,
            onPressed: _isLoading ? null : _startSeedExport,
            loading: _isLoading,
            child: const Text('I understand the risk'),
          ),
          SizedBox(height: PirateSpacing.md),
          PTextButton(
            label: 'Cancel',
            onPressed: () async {
              if (_exportStarted) {
                await FfiBridge.cancelSeedExport();
                _exportStarted = false;
              }
              if (mounted) {
                Navigator.pop(context);
              }
            },
            variant: PTextButtonVariant.subtle,
          ),
        ],
      ),
    );
  }

  Widget _buildWarningCard({
    required IconData icon,
    required String title,
    required String description,
  }) {
    return Container(
      padding: EdgeInsets.all(PirateSpacing.lg),
      decoration: BoxDecoration(
        color: Colors.red.withValues(alpha: 0.1),
        border: Border.all(color: Colors.red.withValues(alpha: 0.3)),
        borderRadius: BorderRadius.circular(16),
      ),
      child: Row(
        children: [
          Icon(icon, color: Colors.red[300], size: 32),
          SizedBox(width: PirateSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: PirateTypography.bodyLarge
                      .copyWith(color: Colors.white, fontWeight: FontWeight.w600),
                ),
                SizedBox(height: PirateSpacing.xs),
                Text(
                  description,
                  style: PirateTypography.body.copyWith(color: Colors.grey[400]),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _startSeedExport() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      if (!_exportStarted) {
        await FfiBridge.startSeedExport(widget.walletId);
        _exportStarted = true;
      }
      await FfiBridge.acknowledgeSeedWarning();
      setState(() => _step1Complete = true);
    } catch (e) {
      setState(() => _error = 'Failed to start export: ${e.toString()}');
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  /// Step 2: Biometric authentication
  Widget _buildBiometricStep() {
    return _centeredStep(
      Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(
            Icons.fingerprint,
            size: 100,
            color: PirateTheme.accentColor,
          ),
          SizedBox(height: PirateSpacing.xl),
          Text(
            'Confirm with biometrics',
            style: PirateTypography.h2.copyWith(color: Colors.white),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.md),
          Text(
            'Use biometrics to continue.',
            style: PirateTypography.body.copyWith(color: Colors.grey[400]),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.xxl),
          if (_error != null)
            Padding(
              padding: EdgeInsets.only(bottom: PirateSpacing.lg),
              child: Text(
                _error!,
                style: PirateTypography.body.copyWith(color: Colors.red[400]),
                textAlign: TextAlign.center,
              ),
            ),
          PButton(
            onPressed: _authenticateBiometric,
            loading: _isLoading,
            child: const Text('Verify'),
          ),
          SizedBox(height: PirateSpacing.md),
          PTextButton(
            label: 'Use passphrase instead',
            onPressed: _isLoading ? null : _skipBiometric,
            variant: PTextButtonVariant.subtle,
          ),
        ],
      ),
    );
  }

  /// Step 3: Passphrase verification
  Widget _buildPassphraseStep() {
    return _centeredStep(
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(height: PirateSpacing.xxl),
          Icon(
            Icons.lock,
            size: 80,
            color: PirateTheme.accentColor,
          ),
          SizedBox(height: PirateSpacing.xl),
          Text(
            'Enter your passphrase',
            style: PirateTypography.h2.copyWith(color: Colors.white),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.md),
          Text(
            'Verify to reveal your recovery phrase.',
            style: PirateTypography.body.copyWith(color: Colors.grey[400]),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.xxl),
          PInput(
            controller: _passphraseController,
            label: 'Passphrase',
            obscureText: true,
            autofocus: true,
            onSubmitted: (_) => _verifyPassphrase(),
          ),
          if (_error != null) ...[
            SizedBox(height: PirateSpacing.md),
            Text(
              _error!,
              style: PirateTypography.body.copyWith(color: Colors.red[400]),
              textAlign: TextAlign.center,
            ),
          ],
          SizedBox(height: PirateSpacing.xxl),
          PButton(
            onPressed: _verifyPassphrase,
            loading: _isLoading,
            child: const Text('Reveal recovery phrase'),
          ),
          SizedBox(height: PirateSpacing.md),
          PTextButton(
            label: 'Cancel',
            onPressed: () async {
              if (_exportStarted) {
                await FfiBridge.cancelSeedExport();
                _exportStarted = false;
              }
              _enableScreenshots();
              if (mounted) {
                Navigator.pop(context);
              }
            },
            variant: PTextButtonVariant.subtle,
          ),
        ],
      ),
    );
  }

  /// Step 4: Seed display with copy button and auto-clear
  Widget _buildSeedDisplay() {
    final countdown = ref.watch(_clipboardCountdownProvider);

    return _centeredStep(
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Recovery phrase',
                style: PirateTypography.h3.copyWith(color: Colors.white),
              ),
              IconButton(
                icon: Icon(Icons.close, color: Colors.white),
                onPressed: _showExitConfirmation,
              ),
            ],
          ),
          SizedBox(height: PirateSpacing.lg),
          Container(
            padding: EdgeInsets.all(PirateSpacing.lg),
            decoration: BoxDecoration(
              color: Colors.grey[900],
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: Colors.grey[800]!),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Write these words down in order.',
                  style: PirateTypography.bodySmall.copyWith(color: Colors.grey[400]),
                ),
                SizedBox(height: PirateSpacing.lg),
                _buildMnemonicGrid(),
              ],
            ),
          ),
          SizedBox(height: PirateSpacing.xl),
          if (countdown != null)
            Container(
              padding: EdgeInsets.all(PirateSpacing.md),
              decoration: BoxDecoration(
                color: Colors.orange.withValues(alpha: 0.1),
                border: Border.all(color: Colors.orange.withValues(alpha: 0.3)),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Row(
                children: [
                  Icon(Icons.timer, color: Colors.orange, size: 20),
                  SizedBox(width: PirateSpacing.sm),
                  Text(
                    'Clipboard clears in ${countdown}s',
                    style: PirateTypography.body.copyWith(color: Colors.orange),
                  ),
                ],
              ),
            ),
          if (countdown == null) ...[
            PButton(
              onPressed: _copyToClipboard,
              child: const Text('Copy to clipboard (clears in 30s)'),
            ),
            SizedBox(height: PirateSpacing.md),
          ],
          SizedBox(height: PirateSpacing.md),
          PButton(onPressed: _confirmSaved, child: const Text('Done, saved offline')),
        ],
      ),
    );
  }

  Widget _buildMnemonicGrid() {
    if (_mnemonic == null) return SizedBox.shrink();

    final words = _mnemonic!.split(' ');
    return LayoutBuilder(
      builder: (context, constraints) {
        final isNarrow = constraints.maxWidth < 360;
        final crossAxisCount = isNarrow ? 2 : 3;
        final aspectRatio = isNarrow ? 2.0 : 2.4;
        return GridView.builder(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: crossAxisCount,
            crossAxisSpacing: PirateSpacing.sm,
            mainAxisSpacing: PirateSpacing.sm,
            childAspectRatio: aspectRatio,
          ),
          itemCount: words.length,
          itemBuilder: (context, index) {
            return Container(
              padding: EdgeInsets.symmetric(
                horizontal: PirateSpacing.sm,
                vertical: PirateSpacing.xs,
              ),
              decoration: BoxDecoration(
                color: Colors.grey[850],
                borderRadius: BorderRadius.circular(8),
              ),
              child: Text(
                '${index + 1}. ${words[index]}',
                style: PirateTypography.bodySmall.copyWith(
                  color: Colors.white,
                  fontFamily: 'monospace',
                ),
                maxLines: 2,
                softWrap: true,
                overflow: TextOverflow.fade,
              ),
            );
          },
        );
      },
    );
  }

  Future<void> _authenticateBiometric() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final canCheck = await _localAuth.canCheckBiometrics;
      if (!canCheck) {
        await FfiBridge.skipSeedBiometric();
        setState(() => _step2Complete = true);
        return;
      }

      final authenticated = await _localAuth.authenticate(
        localizedReason: 'Verify to reveal your recovery phrase',
      );

      if (authenticated) {
        await FfiBridge.completeSeedBiometric(true);
        setState(() => _step2Complete = true);
      } else {
        setState(() => _error = 'Biometric authentication failed');
      }
    } catch (e) {
      setState(() {
        _error = 'Biometric authentication error: ${e.toString()}';
      });
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _skipBiometric() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      await FfiBridge.skipSeedBiometric();
      setState(() => _step2Complete = true);
    } catch (e) {
      setState(() => _error = 'Failed to skip biometrics: ${e.toString()}');
    } finally {
      if (mounted) {
        setState(() => _isLoading = false);
      }
    }
  }

  Future<void> _verifyPassphrase() async {
    if (_passphraseController.text.isEmpty) {
      setState(() => _error = 'Enter your passphrase');
      return;
    }

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final words = await FfiBridge.exportSeedWithPassphrase(
        widget.walletId,
        _passphraseController.text,
      );
      final mnemonic = words.join(' ');

      setState(() {
        _mnemonic = mnemonic;
        _step3Complete = true;
        _seedRevealed = true;
      });
      _passphraseController.clear();
    } catch (e) {
      setState(() => _error = 'Failed to verify passphrase: ${e.toString()}');
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _copyToClipboard() async {
    if (_mnemonic == null) return;

    await Clipboard.setData(ClipboardData(text: _mnemonic!));
    
    // Start 30-second countdown
    ref.read(_clipboardCountdownProvider.notifier).state = 30;
    
    _clipboardTimer?.cancel();
    _clipboardTimer = Timer.periodic(Duration(seconds: 1), (timer) {
      final int? current = ref.read(_clipboardCountdownProvider);
      if (current == null || current <= 1) {
        _clearClipboard();
        timer.cancel();
      } else {
        ref.read(_clipboardCountdownProvider.notifier).state = current - 1;
      }
    });

      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Copied. Clears in 30 seconds.'),
          backgroundColor: Colors.orange[700],
        ),
      );
  }

  Future<void> _clearClipboard() async {
    await Clipboard.setData(ClipboardData(text: ''));
    ref.read(_clipboardCountdownProvider.notifier).state = null;
    _clipboardTimer?.cancel();
  }

  Future<void> _confirmSaved() async {
    final confirmed = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (context) => AlertDialog(
        backgroundColor: Colors.grey[900],
        title: Text(
          'Confirm backup',
          style: TextStyle(color: Colors.white),
        ),
        content: Text(
          'Have you written down your recovery phrase?',
          style: TextStyle(color: Colors.grey[300]),
        ),
        actions: [
          PTextButton(
            label: 'Not yet',
            onPressed: () => Navigator.pop(context, false),
            variant: PTextButtonVariant.subtle,
          ),
          PTextButton(
            label: 'Yes, saved',
            onPressed: () => Navigator.pop(context, true),
          ),
        ],
      ),
    );

    if (confirmed == true) {
      await _clearClipboard();
      if (_exportStarted) {
        await FfiBridge.cancelSeedExport();
        _exportStarted = false;
      }
      _enableScreenshots();
      if (mounted) Navigator.pop(context);
    }
  }

  Future<void> _showExitConfirmation() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: Colors.grey[900],
        title: Text(
          'Exit without saving?',
          style: TextStyle(color: Colors.white),
        ),
        content: Text(
          'Are you sure you want to exit? You will need this phrase to restore this wallet.',
          style: TextStyle(color: Colors.grey[300]),
        ),
        actions: [
          PTextButton(
            label: 'Stay here',
            onPressed: () => Navigator.pop(context, false),
            variant: PTextButtonVariant.subtle,
          ),
          PTextButton(
            label: 'Exit',
            onPressed: () => Navigator.pop(context, true),
            variant: PTextButtonVariant.danger,
          ),
        ],
      ),
    );

    if (confirmed == true) {
      await _clearClipboard();
      if (_exportStarted) {
        await FfiBridge.cancelSeedExport();
        _exportStarted = false;
      }
      _enableScreenshots();
      if (mounted) Navigator.pop(context);
    }
  }

  void _disableScreenshots() {
    // Platform-specific screenshot blocking
    // Android: WindowManager.LayoutParams.FLAG_SECURE
    // iOS: Not directly possible, but we show full-screen warning
    // Implementation via method channel in production
  }

  void _enableScreenshots() {
    // Re-enable screenshots on exit
  }
}
