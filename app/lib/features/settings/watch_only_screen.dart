import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../design/theme.dart';
import '../../design/compat.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../core/ffi/ffi_bridge.dart';

/// Watch-Only Wallet Management Screen
class WatchOnlyScreen extends ConsumerStatefulWidget {
  const WatchOnlyScreen({Key? key}) : super(key: key);

  @override
  ConsumerState<WatchOnlyScreen> createState() => _WatchOnlyScreenState();
}

class _WatchOnlyScreenState extends ConsumerState<WatchOnlyScreen> with SingleTickerProviderStateMixin {
  late TabController _tabController;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 2, vsync: this);
  }

  @override
  void dispose() {
    _tabController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Watch-Only Wallets',
      appBar: const PAppBar(
        title: 'Watch-Only Wallets',
        subtitle: 'Incoming activity only',
        showBackButton: true,
        centerTitle: true,
      ),
      body: Container(
        color: Colors.black,
        child: Column(
          children: [
            Material(
              color: Colors.transparent,
              child: TabBar(
                controller: _tabController,
                indicatorColor: PirateTheme.accentColor,
                labelColor: Colors.white,
                unselectedLabelColor: Colors.grey[500],
                tabs: const [
                  Tab(text: 'Export Viewing Key'),
                  Tab(text: 'Import Viewing Key'),
                ],
              ),
            ),
            Expanded(
              child: TabBarView(
                controller: _tabController,
                children: const [
                  ExportIvkTab(),
                  ImportIvkTab(),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Export viewing key tab
class ExportIvkTab extends ConsumerStatefulWidget {
  const ExportIvkTab({Key? key}) : super(key: key);

  @override
  ConsumerState<ExportIvkTab> createState() => _ExportIvkTabState();
}

class _ExportIvkTabState extends ConsumerState<ExportIvkTab> {
  String? _ivk;
  bool _isLoading = false;
  String? _error;

  @override
  Widget build(BuildContext context) {
    final basePadding = PirateSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: PirateSpacing.lg,
    );
    final padding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return SingleChildScrollView(
      padding: padding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Icon(
            Icons.visibility,
            size: 80,
            color: PirateTheme.accentColor,
          ),
          SizedBox(height: PirateSpacing.xl),
          Text(
            'Export incoming viewing key',
            style: PirateTypography.h2.copyWith(color: Colors.white),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.md),
          Text(
            'Share this viewing key to view incoming activity without spending access.',
            style: PirateTypography.body.copyWith(color: Colors.grey[400]),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.xxl),
          _buildInfoCard(),
          SizedBox(height: PirateSpacing.xl),
          if (_ivk == null) ...[
            PButton(
              onPressed: _exportIvk,
              loading: _isLoading,
              icon: const Icon(Icons.key),
              child: const Text('Export Viewing Key'),
            ),
          ] else ...[
            _buildIvkDisplay(),
            SizedBox(height: PirateSpacing.lg),
            PButton(
              onPressed: _copyIvk,
              icon: const Icon(Icons.copy),
              variant: PButtonVariant.secondary,
              child: const Text('Copy to clipboard'),
            ),
            SizedBox(height: PirateSpacing.md),
            PTextButton(
              label: 'Clear',
              onPressed: () => setState(() => _ivk = null),
              variant: PTextButtonVariant.subtle,
            ),
          ],
          if (_error != null) ...[
            SizedBox(height: PirateSpacing.lg),
            Container(
              padding: EdgeInsets.all(PirateSpacing.md),
              decoration: BoxDecoration(
                color: Colors.red.withValues(alpha: 0.1),
                border: Border.all(color: Colors.red.withValues(alpha: 0.3)),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                _error!,
                style: PirateTypography.body.copyWith(color: Colors.red[400]),
                textAlign: TextAlign.center,
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildInfoCard() {
    return Container(
      padding: EdgeInsets.all(PirateSpacing.lg),
      decoration: BoxDecoration(
        color: Colors.blue.withValues(alpha: 0.1),
        border: Border.all(color: Colors.blue.withValues(alpha: 0.3)),
        borderRadius: BorderRadius.circular(16),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.info, color: Colors.blue, size: 24),
              SizedBox(width: PirateSpacing.sm),
              Expanded(
                child: Text(
                  'About viewing keys',
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: PirateTypography.bodyLarge.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
          SizedBox(height: PirateSpacing.md),
          _buildInfoItem('View incoming transactions'),
          _buildInfoItem('Cannot spend'),
          _buildInfoItem('Useful for accounting'),
          _buildInfoItem('Keep viewing keys private. They reveal incoming history.'),
        ],
      ),
    );
  }

  Widget _buildInfoItem(String text) {
    return Padding(
      padding: EdgeInsets.only(bottom: PirateSpacing.xs),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.circle, color: Colors.blue, size: 6),
          SizedBox(width: PirateSpacing.sm),
          Expanded(
            child: Text(
              text,
              style: PirateTypography.bodySmall.copyWith(color: Colors.grey[400]),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildIvkDisplay() {
    return Container(
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
            'Incoming viewing key',
            style: PirateTypography.bodySmall.copyWith(color: Colors.grey[400]),
          ),
          SizedBox(height: PirateSpacing.sm),
          SelectableText(
            _ivk!,
            style: PirateTypography.bodySmall.copyWith(
              color: Colors.white,
              fontFamily: 'monospace',
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _exportIvk() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final walletId = await FfiBridge.getActiveWallet();
      if (walletId == null) {
        throw StateError('No active wallet');
      }

      final ivk = await FfiBridge.exportIvkSecure(walletId);
      setState(() => _ivk = ivk);
    } catch (e) {
      setState(() => _error = 'Failed to export viewing key: ${e.toString()}');
    } finally {
      setState(() => _isLoading = false);
    }
  }

  Future<void> _copyIvk() async {
    if (_ivk == null) return;

    await Clipboard.setData(ClipboardData(text: _ivk!));
    
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Viewing key copied'),
          backgroundColor: Colors.green[700],
        ),
      );
    }
  }
}

/// Import viewing key tab
class ImportIvkTab extends ConsumerStatefulWidget {
  const ImportIvkTab({Key? key}) : super(key: key);

  @override
  ConsumerState<ImportIvkTab> createState() => _ImportIvkTabState();
}

class _ImportIvkTabState extends ConsumerState<ImportIvkTab> {
  final _nameController = TextEditingController();
  final _ivkController = TextEditingController();
  final _birthdayController = TextEditingController();
  bool _isLoading = false;
  String? _error;

  @override
  void dispose() {
    _nameController.dispose();
    _ivkController.dispose();
    _birthdayController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final basePadding = PirateSpacing.screenPadding(
      MediaQuery.of(context).size.width,
      vertical: PirateSpacing.lg,
    );
    final padding = basePadding.copyWith(
      bottom: basePadding.bottom + MediaQuery.of(context).viewInsets.bottom,
    );
    return SingleChildScrollView(
      padding: padding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Icon(
            Icons.remove_red_eye,
            size: 80,
            color: Colors.orange,
          ),
          SizedBox(height: PirateSpacing.xl),
          Text(
            'Import watch-only wallet',
            style: PirateTypography.h2.copyWith(color: Colors.white),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.md),
          Text(
            'Create a watch-only wallet to view incoming activity.',
            style: PirateTypography.body.copyWith(color: Colors.grey[400]),
            textAlign: TextAlign.center,
          ),
          SizedBox(height: PirateSpacing.xxl),
          _buildWatchOnlyBadge(),
          SizedBox(height: PirateSpacing.xl),
          PInput(
            controller: _nameController,
            label: 'Wallet name',
            hint: 'e.g., Savings (watch-only)',
          ),
          SizedBox(height: PirateSpacing.lg),
          PInput(
            controller: _ivkController,
            label: 'Incoming viewing key',
            hint: 'Paste your viewing key',
            maxLines: 3,
          ),
          SizedBox(height: PirateSpacing.lg),
          PInput(
            controller: _birthdayController,
            label: 'Birthday height (optional)',
            hint: 'e.g., 4000000',
            keyboardType: TextInputType.number,
          ),
          SizedBox(height: PirateSpacing.md),
          Text(
            'Birthday height helps speed up initial sync. Leave blank to use the default height.',
            style: PirateTypography.bodySmall.copyWith(color: Colors.grey[500]),
          ),
          if (_error != null) ...[
            SizedBox(height: PirateSpacing.lg),
            Container(
              padding: EdgeInsets.all(PirateSpacing.md),
              decoration: BoxDecoration(
                color: Colors.red.withValues(alpha: 0.1),
                border: Border.all(color: Colors.red.withValues(alpha: 0.3)),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                _error!,
                style: PirateTypography.body.copyWith(color: Colors.red[400]),
                textAlign: TextAlign.center,
              ),
            ),
          ],
          SizedBox(height: PirateSpacing.xxl),
          PButton(
            onPressed: _importWallet,
            loading: _isLoading,
            icon: const Icon(Icons.add),
            child: const Text('Import watch-only wallet'),
          ),
        ],
      ),
    );
  }

  Widget _buildWatchOnlyBadge() {
    return Container(
      padding: EdgeInsets.all(PirateSpacing.lg),
      decoration: BoxDecoration(
        color: Colors.orange.withValues(alpha: 0.1),
        border: Border.all(color: Colors.orange.withValues(alpha: 0.3)),
        borderRadius: BorderRadius.circular(16),
      ),
      child: Row(
        children: [
          Icon(Icons.visibility_off, color: Colors.orange, size: 32),
          SizedBox(width: PirateSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Incoming only',
                  style: PirateTypography.bodyLarge.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                SizedBox(height: PirateSpacing.xs),
                Text(
                  'Watch-only wallets cannot spend. They only show incoming activity.',
                  style: PirateTypography.bodySmall.copyWith(color: Colors.grey[400]),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _importWallet() async {
    // Validate inputs
    if (_nameController.text.trim().isEmpty) {
      setState(() => _error = 'Please enter a wallet name');
      return;
    }

    if (_ivkController.text.trim().isEmpty) {
      setState(() => _error = 'Please enter a viewing key');
      return;
    }

    final trimmed = _ivkController.text.trim();
    if (!(trimmed.startsWith('zxviews') || trimmed.startsWith('pirate-extended-viewing-key'))) {
      setState(() => _error = 'Invalid viewing key format.');
      return;
    }

    setState(() {
      _isLoading = true;
      _error = null;
    });

    try {
      final birthday = _birthdayController.text.isNotEmpty
          ? int.tryParse(_birthdayController.text)
          : null;

      // Import via FFI
      final walletId = await FfiBridge.importIvk(
        name: _nameController.text.trim(),
        saplingIvk: trimmed,
        birthday: birthday ?? FfiBridge.defaultBirthdayHeight,
      );

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Watch-only wallet imported.'),
            backgroundColor: Colors.green[700],
          ),
        );
        Navigator.pop(context);
      }
    } catch (e) {
      setState(() => _error = 'Failed to import wallet: ${e.toString()}');
    } finally {
      setState(() => _isLoading = false);
    }
  }
}

