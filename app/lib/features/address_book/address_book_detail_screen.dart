/// Address Book Detail Screen
///
/// Displays full details of a saved address with actions:
/// - Copy address
/// - Send to address
/// - Edit entry
/// - Delete entry
/// - QR code display
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../design/deep_space_theme.dart';
import '../../ui/atoms/p_gradient_button.dart';
import '../../ui/atoms/p_icon_button.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import '../../ui/organisms/p_sliver_header.dart';
import 'models/address_entry.dart';
import 'providers/address_book_provider.dart';

/// Address Book Detail Screen
class AddressBookDetailScreen extends ConsumerStatefulWidget {
  const AddressBookDetailScreen({
    required this.entry,
    super.key,
  });

  final AddressEntry entry;

  @override
  ConsumerState<AddressBookDetailScreen> createState() =>
      _AddressBookDetailScreenState();
}

class _AddressBookDetailScreenState extends ConsumerState<AddressBookDetailScreen> {
  bool _showQR = false;
  bool _addressCopied = false;
  late AddressEntry _entry;

  @override
  void initState() {
    super.initState();
    _entry = widget.entry;
  }

  void _copyAddress() {
    Clipboard.setData(ClipboardData(text: _entry.address));
    DeepSpaceHaptics.lightImpact();
    
    setState(() => _addressCopied = true);
    
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          'Address copied',
          style: AppTypography.body.copyWith(color: AppColors.textPrimary),
        ),
        backgroundColor: AppColors.surfaceElevated,
        duration: const Duration(seconds: 2),
      ),
    );
    
    // Reset copied state after 2 seconds
    Future.delayed(const Duration(seconds: 2), () {
      if (mounted) {
        setState(() => _addressCopied = false);
      }
    });
  }

  void _sendToAddress() {
    final encoded = Uri.encodeComponent(_entry.address);
    context.push('/send?address=$encoded');
  }

  void _editEntry() {
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => AddressBookEditScreen(entry: _entry),
      ),
    ).then((result) {
      if (result is AddressEntry) {
        setState(() => _entry = result);
      }
    });
  }

  Future<void> _deleteEntry() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => _DeleteConfirmationDialog(entry: _entry),
    );

    if (confirmed == true) {
      final success = await ref
          .read(addressBookProvider(_entry.walletId).notifier)
          .deleteEntry(_entry.id);
      if (mounted && success) {
        Navigator.of(context).pop(true);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final gutter = AppSpacing.responsiveGutter(MediaQuery.of(context).size.width);
    final sectionPadding =
        AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    return PScaffold(
      title: _entry.label,
      body: CustomScrollView(
        slivers: [
          SliverPersistentHeader(
            pinned: true,
            delegate: PSliverHeaderDelegate(
              maxExtentHeight: 180,
              minExtentHeight: 120,
              builder: (context, shrinkOffset, overlapsContent) {
                final progress =
                    (shrinkOffset / (180 - 120)).clamp(0.0, 1.0);
                return Container(
                  color: AppColors.voidBlack,
                  child: SafeArea(
                    bottom: false,
                    child: Padding(
                      padding: EdgeInsets.only(
                        left: gutter,
                        right: gutter,
                        top: AppSpacing.md,
                        bottom: AppSpacing.md,
                      ),
                      child: Row(
                        children: [
                          PIconButton(
                            icon: Icon(
                              Icons.arrow_back,
                              color: AppColors.textPrimary,
                            ),
                            onPressed: () => Navigator.of(context).pop(),
                            tooltip: 'Back',
                          ),
                          const SizedBox(width: AppSpacing.md),
                          Expanded(
                            child: Opacity(
                              opacity: 1 - (progress * 0.3),
                              child: Text(
                                _entry.label,
                                style: AppTypography.h3.copyWith(
                                  color: AppColors.textPrimary,
                                ),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ),
                          const SizedBox(width: AppSpacing.sm),
                          PIconButton(
                            icon: Icon(
                              Icons.edit_outlined,
                              color: AppColors.textSecondary,
                            ),
                            onPressed: _editEntry,
                            tooltip: 'Edit',
                          ),
                          PIconButton(
                            icon: Icon(
                              Icons.delete_outline,
                              color: AppColors.error,
                            ),
                            onPressed: _deleteEntry,
                            tooltip: 'Delete',
                          ),
                        ],
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
          SliverToBoxAdapter(
            child: Padding(
              padding: sectionPadding,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _buildColorIndicator(),
                  const SizedBox(height: AppSpacing.xl),
                  _buildAddressCard(),
                  const SizedBox(height: AppSpacing.lg),
                  _buildQRSection(),
                  const SizedBox(height: AppSpacing.lg),
                  if (_entry.notes != null && _entry.notes!.isNotEmpty)
                    _buildNotesSection(),
                  const SizedBox(height: AppSpacing.lg),
                  _buildMetadataSection(),
                  const SizedBox(height: AppSpacing.xl),
                  _buildActionButtons(),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildColorIndicator() {
    return Row(
      children: [
        Container(
          width: 16,
          height: 16,
          decoration: BoxDecoration(
            color: _entry.colorTag.color,
            shape: BoxShape.circle,
            boxShadow: [
              BoxShadow(
                color: _entry.colorTag.color.withValues(alpha: 0.4),
                blurRadius: 8,
                spreadRadius: 2,
              ),
            ],
          ),
        ),
        const SizedBox(width: AppSpacing.sm),
        Text(
          _entry.colorTag.displayName,
          style: AppTypography.caption.copyWith(
            color: _entry.colorTag.color,
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
    );
  }

  Widget _buildAddressCard() {
    return Container(
      padding: const EdgeInsets.all(AppSpacing.lg),
      decoration: BoxDecoration(
        color: AppColors.surfaceElevated,
        borderRadius: BorderRadius.circular(16),
        border: Border.all(
          color: AppColors.borderSubtle,
          width: 1,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Shielded address',
            style: AppTypography.caption.copyWith(
              color: AppColors.textMuted,
            ),
          ),
          const SizedBox(height: AppSpacing.sm),
          SelectableText(
            _entry.address,
            style: AppTypography.code.copyWith(
              color: AppColors.textPrimary,
              fontSize: 14,
              height: 1.5,
            ),
          ),
          const SizedBox(height: AppSpacing.md),
          Row(
            children: [
              Expanded(
                child: _ActionChip(
                  icon: _addressCopied ? Icons.check : Icons.copy_outlined,
                  label: _addressCopied ? 'Copied!' : 'Copy',
                  color: _addressCopied
                      ? AppColors.success
                      : AppColors.gradientAStart,
                  onPressed: _copyAddress,
                ),
              ),
              const SizedBox(width: AppSpacing.sm),
              Expanded(
                child: _ActionChip(
                  icon: Icons.qr_code,
                  label: _showQR ? 'Hide QR' : 'Show QR',
                  color: AppColors.textSecondary,
                  onPressed: () => setState(() => _showQR = !_showQR),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildQRSection() {
    return AnimatedCrossFade(
      duration: DeepSpaceDurations.normal,
      crossFadeState: _showQR ? CrossFadeState.showSecond : CrossFadeState.showFirst,
      firstChild: const SizedBox.shrink(),
      secondChild: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(AppSpacing.xl),
        decoration: BoxDecoration(
          color: Colors.white,
          borderRadius: BorderRadius.circular(16),
        ),
        child: Column(
          children: [
            // Placeholder for QR code
            Container(
              width: 200,
              height: 200,
              decoration: BoxDecoration(
                color: Colors.grey[100],
                borderRadius: BorderRadius.circular(8),
              ),
              child: Center(
                child: Icon(
                  Icons.qr_code_2,
                  size: 160,
                  color: Colors.grey[800],
                ),
              ),
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              'Scan to send to this address',
              style: AppTypography.caption.copyWith(
                color: Colors.grey[600],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildNotesSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Notes',
          style: AppTypography.caption.copyWith(
            color: AppColors.textMuted,
          ),
        ),
        const SizedBox(height: AppSpacing.sm),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(AppSpacing.md),
          decoration: BoxDecoration(
            color: AppColors.surfaceElevated,
            borderRadius: BorderRadius.circular(12),
            border: Border.all(
              color: AppColors.borderSubtle,
              width: 1,
            ),
          ),
          child: Text(
            _entry.notes!,
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildMetadataSection() {
    final dateFormat = (DateTime dt) =>
        '${dt.year}-${dt.month.toString().padLeft(2, '0')}-${dt.day.toString().padLeft(2, '0')}';

    return Container(
      padding: const EdgeInsets.all(AppSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.nebula.withValues(alpha: 0.3),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        children: [
          _MetadataRow(
            label: 'Created on',
            value: dateFormat(_entry.createdAt),
          ),
          const SizedBox(height: AppSpacing.sm),
          _MetadataRow(
            label: 'Updated on',
            value: dateFormat(_entry.updatedAt),
          ),
        ],
      ),
    );
  }

  Widget _buildActionButtons() {
    return PGradientButton(
      text: 'Send to ${_entry.label}',
      icon: Icons.send,
      fullWidth: true,
      onPressed: _sendToAddress,
    );
  }
}

// =============================================================================
// Supporting Widgets
// =============================================================================

class _ActionChip extends StatelessWidget {
  const _ActionChip({
    required this.icon,
    required this.label,
    required this.color,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final Color color;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onPressed,
        borderRadius: BorderRadius.circular(8),
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.md,
            vertical: AppSpacing.sm,
          ),
          decoration: BoxDecoration(
            color: color.withValues(alpha: 0.1),
            borderRadius: BorderRadius.circular(8),
            border: Border.all(
              color: color.withValues(alpha: 0.3),
              width: 1,
            ),
          ),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(
                icon,
                size: 18,
                color: color,
              ),
              const SizedBox(width: AppSpacing.xs),
              Text(
                label,
                style: AppTypography.caption.copyWith(
                  color: color,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _MetadataRow extends StatelessWidget {
  const _MetadataRow({
    required this.label,
    required this.value,
  });

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: Text(
            label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: AppTypography.caption.copyWith(
              color: AppColors.textMuted,
            ),
          ),
        ),
        const SizedBox(width: AppSpacing.sm),
        Expanded(
          child: Text(
            value,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.right,
            style: AppTypography.caption.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
        ),
      ],
    );
  }
}

class _DeleteConfirmationDialog extends StatelessWidget {
  const _DeleteConfirmationDialog({required this.entry});

  final AddressEntry entry;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      backgroundColor: AppColors.surfaceElevated,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(20),
      ),
      title: Text(
        'Delete address?',
        style: AppTypography.h3.copyWith(
          color: AppColors.textPrimary,
        ),
      ),
      content: Text(
        'Delete "${entry.label}"? This cannot be undone.',
        style: AppTypography.body.copyWith(
          color: AppColors.textSecondary,
        ),
      ),
      actions: [
        PTextButton(
          label: 'Cancel',
          onPressed: () => Navigator.of(context).pop(false),
          variant: PTextButtonVariant.subtle,
        ),
        PTextButton(
          label: 'Delete',
          onPressed: () => Navigator.of(context).pop(true),
          variant: PTextButtonVariant.danger,
        ),
      ],
    );
  }
}

// =============================================================================
// Edit Screen (Full Implementation)
// =============================================================================

class AddressBookEditScreen extends ConsumerStatefulWidget {
  const AddressBookEditScreen({this.entry, this.walletId, super.key})
      : assert(entry != null || walletId != null);

  final AddressEntry? entry;
  final String? walletId;

  @override
  ConsumerState<AddressBookEditScreen> createState() =>
      _AddressBookEditScreenState();
}

class _AddressBookEditScreenState extends ConsumerState<AddressBookEditScreen> {
  final _formKey = GlobalKey<FormState>();
  late TextEditingController _labelController;
  late TextEditingController _addressController;
  late TextEditingController _notesController;
  ColorTag _selectedColor = ColorTag.none;
  bool _isSaving = false;

  bool get _isEditing => widget.entry != null;
  String get _walletId => widget.entry?.walletId ?? widget.walletId!;

  @override
  void initState() {
    super.initState();
    _labelController = TextEditingController(text: widget.entry?.label ?? '');
    _addressController = TextEditingController(text: widget.entry?.address ?? '');
    _notesController = TextEditingController(text: widget.entry?.notes ?? '');
    _selectedColor = widget.entry?.colorTag ?? ColorTag.none;
  }

  @override
  void dispose() {
    _labelController.dispose();
    _addressController.dispose();
    _notesController.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (!_formKey.currentState!.validate()) return;

    setState(() => _isSaving = true);
    final notifier = ref.read(addressBookProvider(_walletId).notifier);

    try {
      if (_isEditing) {
        final updated = widget.entry!.copyWith(
          label: _labelController.text.trim(),
          notes: _notesController.text.trim().isEmpty
              ? null
              : _notesController.text.trim(),
          colorTag: _selectedColor,
        );

        final success = await notifier.updateEntry(updated);
        if (mounted && success) {
          Navigator.of(context).pop(updated);
        } else if (mounted) {
          _showError(ref.read(addressBookProvider(_walletId)).error);
        }
      } else {
        final newEntry = await notifier.addEntry(
          address: _addressController.text.trim(),
          label: _labelController.text.trim(),
          notes: _notesController.text.trim().isEmpty
              ? null
              : _notesController.text.trim(),
          colorTag: _selectedColor,
        );

        if (mounted && newEntry != null) {
          Navigator.of(context).pop(newEntry);
        } else if (mounted) {
          _showError(ref.read(addressBookProvider(_walletId)).error);
        }
      }
    } catch (e) {
      if (mounted) {
        _showError(e.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _isSaving = false);
      }
    }
  }

  void _showError(String? message) {
    final text = message ?? 'Failed to save address';
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(text),
        backgroundColor: AppColors.error,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      appBar: PAppBar(
        title: _isEditing ? 'Edit Address' : 'Add Address',
        subtitle: 'Edit label, notes, and color',
        onBack: () => Navigator.of(context).pop(),
        actions: [
          if (_isSaving)
            const Padding(
              padding: EdgeInsets.only(right: AppSpacing.sm),
              child: SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            )
          else
            PTextButton(
              label: 'Save',
              onPressed: _save,
            ),
        ],
      ),
      body: Form(
        key: _formKey,
        child: ListView(
          padding: const EdgeInsets.all(AppSpacing.lg),
          children: [
            // Label field
            _buildTextField(
              controller: _labelController,
              label: 'Label',
              hint: 'e.g., Alice, Cold Storage',
              validator: (value) {
                if (value == null || value.trim().isEmpty) {
                  return 'Please enter a label';
                }
                if (value.length > kMaxLabelLength) {
                  return 'Label must be $kMaxLabelLength characters or less';
                }
                return null;
              },
            ),

            const SizedBox(height: AppSpacing.lg),

            // Address field
            _buildTextField(
              controller: _addressController,
              label: 'Shielded address',
              hint: 'zs1...',
              maxLines: 3,
              enabled: !_isEditing,
              validator: (value) {
                if (value == null || value.trim().isEmpty) {
                  return 'Please enter an address';
                }
                if (!value.startsWith('zs1')) {
                  return 'Address must be a Sapling address (zs1...)';
                }
                if (value.length < 70) {
                  return 'Invalid address length';
                }
                return null;
              },
            ),

            const SizedBox(height: AppSpacing.lg),

            // Notes field
            _buildTextField(
              controller: _notesController,
              label: 'Notes (optional)',
              hint: 'Add notes for this address',
              maxLines: 4,
              validator: (value) {
                if (value != null && value.length > kMaxNotesLength) {
                  return 'Notes must be $kMaxNotesLength characters or less';
                }
                return null;
              },
            ),

            const SizedBox(height: AppSpacing.xl),

            // Color tag selector
            _buildColorSelector(),

            const SizedBox(height: AppSpacing.xxl),

            // Paste from clipboard button
            if (!_isEditing)
              OutlinedButton.icon(
                onPressed: () async {
                  final data = await Clipboard.getData(Clipboard.kTextPlain);
                  if (data?.text != null) {
                    _addressController.text = data!.text!;
                  }
                },
                icon: Icon(
                  Icons.paste,
                  color: AppColors.textSecondary,
                ),
                label: Text(
                  'Paste from clipboard',
                  style: AppTypography.body.copyWith(
                    color: AppColors.textSecondary,
                  ),
                ),
                style: OutlinedButton.styleFrom(
                  padding: const EdgeInsets.all(AppSpacing.md),
                  side: BorderSide(
                    color: AppColors.borderSubtle,
                  ),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildTextField({
    required TextEditingController controller,
    required String label,
    required String hint,
    int maxLines = 1,
    bool enabled = true,
    String? Function(String?)? validator,
  }) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: AppTypography.caption.copyWith(
            color: AppColors.textSecondary,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: AppSpacing.sm),
        TextFormField(
          controller: controller,
          maxLines: maxLines,
          enabled: enabled,
          style: maxLines > 1
              ? AppTypography.code.copyWith(
                  color: AppColors.textPrimary,
                  fontSize: 14,
                )
              : AppTypography.body.copyWith(
                  color: AppColors.textPrimary,
                ),
          validator: validator,
          decoration: InputDecoration(
            hintText: hint,
            hintStyle: AppTypography.body.copyWith(
              color: AppColors.textMuted,
            ),
            filled: true,
            fillColor: AppColors.surfaceElevated,
            border: OutlineInputBorder(
              borderRadius: BorderRadius.circular(12),
              borderSide: BorderSide(
                color: AppColors.borderSubtle,
              ),
            ),
            enabledBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(12),
              borderSide: BorderSide(
                color: AppColors.borderSubtle,
              ),
            ),
            focusedBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(12),
              borderSide: BorderSide(
                color: AppColors.gradientAStart,
                width: 2,
              ),
            ),
            errorBorder: OutlineInputBorder(
              borderRadius: BorderRadius.circular(12),
              borderSide: BorderSide(
                color: AppColors.error,
              ),
            ),
            contentPadding: const EdgeInsets.all(AppSpacing.md),
          ),
        ),
      ],
    );
  }

  Widget _buildColorSelector() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Color Tag',
          style: AppTypography.caption.copyWith(
            color: AppColors.textSecondary,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: AppSpacing.sm),
        Wrap(
          spacing: AppSpacing.sm,
          runSpacing: AppSpacing.sm,
          children: ColorTag.values.map((color) {
            final isSelected = _selectedColor == color;
            return GestureDetector(
              onTap: () => setState(() => _selectedColor = color),
              child: AnimatedContainer(
                duration: DeepSpaceDurations.fast,
                padding: const EdgeInsets.symmetric(
                  horizontal: AppSpacing.md,
                  vertical: AppSpacing.sm,
                ),
                decoration: BoxDecoration(
                  color: isSelected
                      ? color.color.withValues(alpha: 0.2)
                      : AppColors.surfaceElevated,
                  borderRadius: BorderRadius.circular(20),
                  border: Border.all(
                    color: isSelected ? color.color : AppColors.borderSubtle,
                    width: isSelected ? 2 : 1,
                  ),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (color != ColorTag.none) ...[
                      Container(
                        width: 14,
                        height: 14,
                        decoration: BoxDecoration(
                          color: color.color,
                          shape: BoxShape.circle,
                        ),
                      ),
                      const SizedBox(width: AppSpacing.xs),
                    ],
                    Text(
                      color.displayName,
                      style: AppTypography.caption.copyWith(
                        color: isSelected
                            ? color.color
                            : AppColors.textSecondary,
                        fontWeight:
                            isSelected ? FontWeight.w600 : FontWeight.normal,
                      ),
                    ),
                  ],
                ),
              ),
            );
          }).toList(),
        ),
      ],
    );
  }
}

