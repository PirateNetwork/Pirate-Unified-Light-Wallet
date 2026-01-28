/// Address Book screen - Manage saved addresses with labels, notes, and color tags
library;

import 'dart:async';
import 'dart:io' show Platform;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:mobile_scanner/mobile_scanner.dart' as mobile_scanner;
import 'package:zxing2/qrcode.dart';

import '../../core/ffi/ffi_bridge.dart' hide kMaxLabelLength, kMaxNotesLength;
import '../../design/deep_space_theme.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_input.dart';
import '../../ui/atoms/p_text_button.dart';
import '../../ui/molecules/p_bottom_sheet.dart';
import '../../ui/molecules/p_card.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../ui/organisms/p_app_bar.dart';
import '../../ui/organisms/p_scaffold.dart';
import 'models/address_entry.dart';
import 'providers/address_book_provider.dart';
import '../../core/providers/wallet_providers.dart';

/// Address Book screen
class AddressBookScreen extends ConsumerStatefulWidget {
  /// Optional callback when address is selected (for send flow)
  final void Function(AddressEntry)? onSelectAddress;

  const AddressBookScreen({
    super.key,
    this.onSelectAddress,
  });

  @override
  ConsumerState<AddressBookScreen> createState() => _AddressBookScreenState();
}

class _AddressBookScreenState extends ConsumerState<AddressBookScreen> {
  final TextEditingController _searchController = TextEditingController();
  String? _walletId;

  @override
  void initState() {
    super.initState();
    _searchController.addListener(_onSearchChanged);
  }

  @override
  void dispose() {
    _searchController.dispose();
    super.dispose();
  }

  void _onSearchChanged() {
    final walletId = _walletId ?? ref.read(activeWalletProvider);
    if (walletId == null) return;
    ref.read(addressBookProvider(walletId).notifier)
        .setSearchQuery(_searchController.text);
  }

  void _showAddSheet() {
    final walletId = _walletId;
    if (walletId == null) {
      _showSnackBar('No active wallet');
      return;
    }
    PBottomSheet.show<void>(
      context: context,
      title: 'Add Address',
      content: AddEditAddressSheet(
        walletId: walletId,
        onSave: (entry) {
          Navigator.of(context).pop();
          _showSnackBar('Address saved');
        },
      ),
    );
  }

  void _showEditSheet(AddressEntry entry) {
    final walletId = _walletId;
    if (walletId == null) {
      _showSnackBar('No active wallet');
      return;
    }
    PBottomSheet.show<void>(
      context: context,
      title: 'Edit Address',
      content: AddEditAddressSheet(
        walletId: walletId,
        entry: entry,
        onSave: (updated) {
          Navigator.of(context).pop();
          _showSnackBar('Address updated');
        },
      ),
    );
  }

  void _showDetailsSheet(AddressEntry entry) {
    PBottomSheet.show<void>(
      context: context,
      title: 'Address Details',
      content: AddressDetailsSheet(
        entry: entry,
        onEdit: () {
          Navigator.of(context).pop();
          _showEditSheet(entry);
        },
        onDelete: () async {
          Navigator.of(context).pop();
          await _confirmDelete(entry);
        },
        onSend: () {
          Navigator.of(context).pop();
          if (widget.onSelectAddress != null) {
            widget.onSelectAddress!(entry);
          } else {
            context.push('/send?address=${Uri.encodeComponent(entry.address)}');
          }
        },
      ),
    );
  }

  Future<void> _confirmDelete(AddressEntry entry) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => PDialog(
        title: 'Delete Address?',
        content: Text(
          'Are you sure you want to delete "${entry.label}" from your address book?',
          style: AppTypography.body,
        ),
        actions: [
          PDialogAction<bool>(
            label: 'Cancel',
            onPressed: () => Navigator.of(context).pop(false),
            variant: PButtonVariant.secondary,
            result: false,
          ),
          PDialogAction<bool>(
            label: 'Delete',
            onPressed: () => Navigator.of(context).pop(true),
            variant: PButtonVariant.primary,
            result: true,
          ),
        ],
      ),
    );

    if (confirmed ?? false) {
      final success = await ref
          .read(addressBookProvider(entry.walletId).notifier)
          .deleteEntry(entry.id);
      if (success) {
        _showSnackBar('Address deleted');
      }
    }
  }

  void _showSnackBar(String message) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(message)),
    );
  }

  void _showFilterSheet() {
    final walletId = _walletId;
    if (walletId == null) {
      _showSnackBar('No active wallet');
      return;
    }
    final state = ref.read(addressBookProvider(walletId));

    PBottomSheet.show<void>(
      context: context,
      title: 'Filter',
      content: FilterSheet(
        currentColor: state.filterColor,
        showFavoritesOnly: state.showFavoritesOnly,
        colorCounts: state.colorCounts,
        onColorChanged: (color) {
          ref.read(addressBookProvider(walletId).notifier)
              .setColorFilter(color);
          Navigator.of(context).pop();
        },
        onFavoritesToggled: () {
          ref.read(addressBookProvider(walletId).notifier)
              .toggleFavoritesFilter();
          Navigator.of(context).pop();
        },
        onClear: () {
          ref.read(addressBookProvider(walletId).notifier).clearFilters();
          _searchController.clear();
          Navigator.of(context).pop();
        },
      ),
    );
  }

  Widget _buildFilterIcon(bool hasFilters) {
    final baseIcon = Icon(
      Icons.filter_list,
      color: AppColors.textPrimary,
    );

    if (!hasFilters) return baseIcon;

    return Stack(
      clipBehavior: Clip.none,
      children: [
        baseIcon,
        Positioned(
          top: -2,
          right: -2,
          child: Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(
              color: AppColors.accentPrimary,
              shape: BoxShape.circle,
            ),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    // Listen for wallet changes
    ref.listen<WalletId?>(activeWalletProvider, (previous, next) {
      if (!mounted || next == _walletId) return;
      setState(() => _walletId = next);
      if (next != null) {
        ref.read(addressBookProvider(next).notifier)
            .setSearchQuery(_searchController.text);
      }
    });

    // Read current wallet if not initialized
    _walletId ??= ref.read(activeWalletProvider);
    
    final walletId = _walletId;
    if (walletId == null) {
      return PScaffold(
        title: 'Address Book',
        appBar: const PAppBar(
          title: 'Address Book',
          subtitle: 'Manage your trusted contacts',
          actions: [WalletSwitcherButton(compact: true)],
        ),
        body: const Center(
          child: Text('No active wallet'),
        ),
      );
    }

    final state = ref.watch(addressBookProvider(walletId));
    final filteredEntries = state.filteredEntries;
    final hasFilters = state.searchQuery.isNotEmpty ||
        state.filterColor != null ||
        state.showFavoritesOnly;
    final gutter = AppSpacing.responsiveGutter(MediaQuery.of(context).size.width);

    return PScaffold(
      title: 'Address Book',
      appBar: PAppBar(
        title: 'Address Book',
        subtitle: widget.onSelectAddress != null
            ? 'Tap an entry to autofill the send form'
            : 'Manage your trusted contacts',
        actions: [
          const WalletSwitcherButton(compact: true),
          PIconButton(
            icon: _buildFilterIcon(hasFilters),
            onPressed: _showFilterSheet,
            tooltip: 'Filters',
          ),
        ],
      ),
      body: Column(
        children: [
          // Search bar
          Padding(
            padding: EdgeInsets.fromLTRB(
              gutter,
              AppSpacing.lg,
              gutter,
              AppSpacing.md,
            ),
            child: PInput(
              controller: _searchController,
              hint: 'Search addresses...',
              prefixIcon: const Icon(Icons.search),
              suffixIcon: state.searchQuery.isNotEmpty
                  ? IconButton(
                      icon: const Icon(Icons.clear),
                      onPressed: _searchController.clear,
                      tooltip: 'Clear',
                    )
                  : null,
            ),
          ),

          // Loading state
          if (state.isLoading)
            const Expanded(
              child: Center(child: CircularProgressIndicator()),
            )
          // Error state
          else if (state.error != null)
            Expanded(
              child: Center(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Icon(
                      Icons.error_outline,
                      size: 64,
                      color: AppColors.error,
                    ),
                    const SizedBox(height: AppSpacing.md),
                    Text(
                      state.error!,
                      style: AppTypography.body.copyWith(
                        color: AppColors.textSecondary,
                      ),
                      textAlign: TextAlign.center,
                    ),
                    const SizedBox(height: AppSpacing.lg),
                    PButton(
                      text: 'Retry',
                      onPressed: () {
                        ref.read(addressBookProvider(walletId).notifier)
                            .refresh();
                      },
                      variant: PButtonVariant.secondary,
                    ),
                  ],
                ),
              ),
            )
          // Empty state
          else if (filteredEntries.isEmpty)
            Expanded(
              child: EmptyAddressBookState(
                hasSearch: hasFilters,
                onClearFilters: () {
                  ref.read(addressBookProvider(walletId).notifier)
                      .clearFilters();
                  _searchController.clear();
                },
              ),
            )
          // Address list
          else
            Expanded(
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(
                  horizontal: AppSpacing.lg,
                  vertical: AppSpacing.md,
                ),
                itemCount: filteredEntries.length,
                separatorBuilder: (_, _) =>
                    const SizedBox(height: AppSpacing.md),
                itemBuilder: (context, index) {
                  final entry = filteredEntries[index];
                  return AddressCard(
                    key: ValueKey(entry.id),
                    entry: entry,
                    onTap: () => _showDetailsSheet(entry),
                    onFavoriteToggle: () {
                      ref.read(addressBookProvider(walletId).notifier)
                          .toggleFavorite(entry.id);
                    },
                  );
                },
              ),
            ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _showAddSheet,
        icon: const Icon(Icons.add),
        label: const Text('Add Address'),
        backgroundColor: AppColors.accentPrimary,
      ),
    );
  }
}

/// Address card widget
class AddressCard extends StatelessWidget {
  final AddressEntry entry;
  final VoidCallback onTap;
  final VoidCallback onFavoriteToggle;

  const AddressCard({
    super.key,
    required this.entry,
    required this.onTap,
    required this.onFavoriteToggle,
  });

  @override
  Widget build(BuildContext context) {
    return PCard(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(16),
        child: Padding(
          padding: const EdgeInsets.all(AppSpacing.md),
          child: Row(
            children: [
              // Avatar with color tag
              Stack(
                children: [
                  CircleAvatar(
                    radius: 24,
                    backgroundColor: entry.colorTag.color.withValues(alpha: 0.2),
                    child: Text(
                      entry.avatarLetter,
                      style: AppTypography.h4.copyWith(
                        color: entry.colorTag.color,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                  if (entry.colorTag != ColorTag.none)
                    Positioned(
                      right: 0,
                      bottom: 0,
                      child: Container(
                        width: 12,
                        height: 12,
                        decoration: BoxDecoration(
                          color: entry.colorTag.color,
                          shape: BoxShape.circle,
                          border: Border.all(
                            color: AppColors.surface,
                            width: 2,
                          ),
                        ),
                      ),
                    ),
                ],
              ),

              const SizedBox(width: AppSpacing.md),

              // Details
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            entry.label,
                            style: AppTypography.bodyBold.copyWith(
                              color: AppColors.textPrimary,
                            ),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (entry.isFavorite)
                          Icon(
                            Icons.star,
                            size: 16,
                            color: AppColors.warning,
                          ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Text(
                      entry.truncatedAddress,
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                        fontFamily: 'monospace',
                      ),
                    ),
                    if (entry.notes != null && entry.notes!.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      Text(
                        entry.notes!,
                        style: AppTypography.caption.copyWith(
                          color: AppColors.textTertiary,
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ],
                ),
              ),

              // Favorite button
              IconButton(
                icon: Icon(
                  entry.isFavorite ? Icons.star : Icons.star_border,
                  color: entry.isFavorite
                      ? AppColors.warning
                      : AppColors.textTertiary,
                ),
                onPressed: onFavoriteToggle,
                tooltip: entry.isFavorite
                    ? 'Remove from favorites'
                    : 'Add to favorites',
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Empty state widget
class EmptyAddressBookState extends StatelessWidget {
  final bool hasSearch;
  final VoidCallback onClearFilters;

  const EmptyAddressBookState({
    super.key,
    required this.hasSearch,
    required this.onClearFilters,
  });

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              hasSearch ? Icons.search_off : Icons.contacts_outlined,
              size: 80,
              color: AppColors.textTertiary,
            ),
            const SizedBox(height: AppSpacing.lg),
            Text(
              hasSearch ? 'No Results Found' : 'No Saved Addresses',
              style: AppTypography.h3.copyWith(
                color: AppColors.textPrimary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              hasSearch
                  ? 'Try a different search or clear filters'
                  : 'Add addresses to quickly send ARRR to your contacts',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            if (hasSearch) ...[
              const SizedBox(height: AppSpacing.lg),
              PButton(
                text: 'Clear Filters',
                onPressed: onClearFilters,
                variant: PButtonVariant.secondary,
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Add/Edit address bottom sheet
class AddEditAddressSheet extends ConsumerStatefulWidget {
  final String walletId;
  final AddressEntry? entry;
  final void Function(AddressEntry) onSave;

  const AddEditAddressSheet({
    super.key,
    required this.walletId,
    this.entry,
    required this.onSave,
  });

  @override
  ConsumerState<AddEditAddressSheet> createState() => _AddEditAddressSheetState();
}

class _AddEditAddressSheetState extends ConsumerState<AddEditAddressSheet> {
  late TextEditingController _labelController;
  late TextEditingController _addressController;
  late TextEditingController _notesController;
  late ColorTag _selectedColor;
  bool _isSaving = false;
  String? _error;

  bool get _isEditing => widget.entry != null;
  bool get _supportsCameraScan => Platform.isAndroid || Platform.isIOS;
  bool get _supportsImageImport => !_supportsCameraScan;

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

  bool _canSave() {
    return _labelController.text.isNotEmpty &&
        _addressController.text.isNotEmpty &&
        !_isSaving;
  }

  String _normalizeQrResult(String value) {
    final trimmed = value.trim();
    if (trimmed.toLowerCase().startsWith('pirate:')) {
      final uri = Uri.parse(trimmed);
      return uri.path.isNotEmpty ? uri.path : trimmed;
    }
    return trimmed;
  }

  Future<void> _scanQr() async {
    if (_supportsCameraScan) {
      final result = await Navigator.of(context).push<String>(
        MaterialPageRoute(
          builder: (_) => const _AddressQrScannerScreen(),
          fullscreenDialog: true,
        ),
      );
      if (result == null || result.isEmpty) return;
      _addressController.text = _normalizeQrResult(result);
      setState(() {});
      return;
    }

    if (_supportsImageImport) {
      final file = await openFile(
        acceptedTypeGroups: [
          const XTypeGroup(
            label: 'Images',
            extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'],
          ),
        ],
      );
      if (file == null) return;

      final bytes = await file.readAsBytes();
      final result = await _decodeQrFromImageBytes(bytes);
      bytes.fillRange(0, bytes.length, 0);

      if (!mounted) return;
      if (result == null || result.isEmpty) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('No QR code found in that image'),
            duration: Duration(seconds: 2),
          ),
        );
        return;
      }

      _addressController.text = _normalizeQrResult(result);
      setState(() {});
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('QR code imported'),
          duration: Duration(seconds: 2),
        ),
      );
    }
  }

  Future<String?> _decodeQrFromImageBytes(Uint8List bytes) async {
    try {
      final uiImage = await _decodeUiImage(bytes);
      final byteData =
          await uiImage.toByteData(format: ui.ImageByteFormat.rawRgba);
      if (byteData == null) {
        uiImage.dispose();
        return null;
      }

      final rgba = byteData.buffer.asUint8List(
        byteData.offsetInBytes,
        byteData.lengthInBytes,
      );
      final pixels = _rgbaToArgbPixels(rgba, uiImage.width, uiImage.height);
      uiImage.dispose();

      final source = RGBLuminanceSource(uiImage.width, uiImage.height, pixels);
      final bitmap = BinaryBitmap(HybridBinarizer(source));
      final reader = QRCodeReader();
      final result = reader.decode(bitmap);
      return result.text;
    } catch (_) {
      return null;
    }
  }

  Future<ui.Image> _decodeUiImage(Uint8List bytes) {
    final completer = Completer<ui.Image>();
    ui.decodeImageFromList(bytes, completer.complete);
    return completer.future;
  }

  Int32List _rgbaToArgbPixels(
    Uint8List rgbaBytes,
    int width,
    int height,
  ) {
    final pixels = Int32List(width * height);
    for (int i = 0; i < pixels.length; i++) {
      final offset = i * 4;
      final r = rgbaBytes[offset];
      final g = rgbaBytes[offset + 1];
      final b = rgbaBytes[offset + 2];
      final a = rgbaBytes[offset + 3];
      pixels[i] = (a << 24) | (r << 16) | (g << 8) | b;
    }
    return pixels;
  }

  Future<void> _save() async {
    if (!_canSave()) return;

    setState(() {
      _isSaving = true;
      _error = null;
    });

    final notifier = ref.read(addressBookProvider(widget.walletId).notifier);

    if (_isEditing) {
      final updated = widget.entry!.copyWith(
        label: _labelController.text.trim(),
        notes: _notesController.text.trim().isEmpty
            ? null
            : _notesController.text.trim(),
        colorTag: _selectedColor,
      );

      final success = await notifier.updateEntry(updated);
      if (success) {
        widget.onSave(updated);
      } else {
        setState(() {
          _error = ref.read(addressBookProvider(widget.walletId)).error;
          _isSaving = false;
        });
      }
    } else {
      final entry = await notifier.addEntry(
        address: _addressController.text.trim(),
        label: _labelController.text.trim(),
        notes: _notesController.text.trim().isEmpty
            ? null
            : _notesController.text.trim(),
        colorTag: _selectedColor,
      );

      if (entry != null) {
        widget.onSave(entry);
      } else {
        setState(() {
          _error = ref.read(addressBookProvider(widget.walletId)).error;
          _isSaving = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(
        bottom: MediaQuery.of(context).viewInsets.bottom,
        left: AppSpacing.lg,
        right: AppSpacing.lg,
        top: AppSpacing.lg,
      ),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              _isEditing ? 'Edit Address' : 'Add Address',
              style: AppTypography.h3.copyWith(
                color: AppColors.textPrimary,
              ),
            ),

            const SizedBox(height: AppSpacing.xl),

            if (_error != null) ...[
              Container(
                padding: const EdgeInsets.all(AppSpacing.md),
                decoration: BoxDecoration(
                  color: AppColors.error.withValues(alpha: 0.1),
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Row(
                  children: [
                    Icon(Icons.error_outline, color: AppColors.error, size: 20),
                    const SizedBox(width: AppSpacing.sm),
                    Expanded(
                      child: Text(
                        _error!,
                        style: AppTypography.caption.copyWith(
                          color: AppColors.error,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: AppSpacing.md),
            ],

            PInput(
              controller: _labelController,
              label: 'Label *',
              hint: 'e.g., Alice, Coffee Shop',
              maxLength: kMaxLabelLength,
              onChanged: (_) => setState(() {}),
            ),

            const SizedBox(height: AppSpacing.md),

            PInput(
              controller: _addressController,
              label: 'Address *',
              hint: 'zs1...',
              maxLines: 3,
              enabled: !_isEditing, // Can't change address when editing
              onChanged: (_) => setState(() {}),
              suffixIcon: _isEditing
                  ? null
                  : Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        IconButton(
                          icon: const Icon(Icons.content_paste, size: 20),
                          onPressed: () async {
                            final data = await Clipboard.getData('text/plain');
                            if (data?.text != null) {
                              _addressController.text = data!.text!;
                              setState(() {});
                            }
                          },
                          tooltip: 'Paste',
                        ),
                        IconButton(
                          icon: const Icon(Icons.qr_code_scanner, size: 20),
                          onPressed: _scanQr,
                          tooltip: _supportsCameraScan ? 'Scan QR' : 'Import QR',
                        ),
                      ],
                    ),
            ),

            const SizedBox(height: AppSpacing.md),

            PInput(
              controller: _notesController,
              label: 'Notes (Optional)',
              hint: 'Add a note about this address',
              maxLines: 2,
              maxLength: kMaxNotesLength,
            ),

            const SizedBox(height: AppSpacing.md),

            // Color tag selector
            Text(
              'Color Tag',
              style: AppTypography.labelMedium.copyWith(
                color: AppColors.textSecondary,
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
                  child: Container(
                    width: 36,
                    height: 36,
                    decoration: BoxDecoration(
                      color: color.color,
                      shape: BoxShape.circle,
                      border: isSelected
                          ? Border.all(color: AppColors.textPrimary, width: 3)
                          : null,
                      boxShadow: isSelected
                          ? [
                              BoxShadow(
                                color: color.color.withValues(alpha: 0.4),
                                blurRadius: 8,
                                spreadRadius: 2,
                              )
                            ]
                          : null,
                    ),
                    child: isSelected
                        ? const Icon(Icons.check, color: Colors.white, size: 20)
                        : null,
                  ),
                );
              }).toList(),
            ),

            const SizedBox(height: AppSpacing.xl),

            Row(
              children: [
                Expanded(
                  child: PButton(
                    text: 'Cancel',
                    onPressed: () => Navigator.of(context).pop(),
                    variant: PButtonVariant.secondary,
                    size: PButtonSize.large,
                  ),
                ),
                const SizedBox(width: AppSpacing.md),
                Expanded(
                  child: PButton(
                    text: _isSaving ? 'Saving...' : 'Save',
                    onPressed: _canSave() ? _save : null,
                    variant: PButtonVariant.primary,
                    size: PButtonSize.large,
                    loading: _isSaving,
                  ),
                ),
              ],
            ),

            const SizedBox(height: AppSpacing.lg),
          ],
        ),
      ),
    );
  }
}

/// Address details bottom sheet
class AddressDetailsSheet extends StatelessWidget {
  final AddressEntry entry;
  final VoidCallback onEdit;
  final VoidCallback onDelete;
  final VoidCallback onSend;

  const AddressDetailsSheet({
    super.key,
    required this.entry,
    required this.onEdit,
    required this.onDelete,
    required this.onSend,
  });

  @override
  Widget build(BuildContext context) {
    final padding = AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    return Padding(
      padding: padding,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Header
          Row(
            children: [
              CircleAvatar(
                radius: 24,
                backgroundColor: entry.colorTag.color.withValues(alpha: 0.2),
                child: Text(
                  entry.avatarLetter,
                  style: AppTypography.h4.copyWith(
                    color: entry.colorTag.color,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ),
              const SizedBox(width: AppSpacing.md),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            entry.label,
                            style: AppTypography.h3.copyWith(
                              color: AppColors.textPrimary,
                            ),
                          ),
                        ),
                        if (entry.isFavorite)
                          Icon(Icons.star, color: AppColors.warning),
                      ],
                    ),
                    if (entry.colorTag != ColorTag.none)
                      Container(
                        margin: const EdgeInsets.only(top: 4),
                        padding: const EdgeInsets.symmetric(
                          horizontal: 8,
                          vertical: 2,
                        ),
                        decoration: BoxDecoration(
                          color: entry.colorTag.color.withValues(alpha: 0.2),
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: Text(
                          entry.colorTag.displayName,
                          style: AppTypography.caption.copyWith(
                            color: entry.colorTag.color,
                          ),
                        ),
                      ),
                  ],
                ),
              ),
              IconButton(
                icon: const Icon(Icons.edit_outlined),
                onPressed: onEdit,
                tooltip: 'Edit',
              ),
              IconButton(
                icon: Icon(Icons.delete_outline, color: AppColors.error),
                onPressed: onDelete,
                tooltip: 'Delete',
              ),
            ],
          ),

          const SizedBox(height: AppSpacing.lg),

          // Address
          _DetailItem(
            label: 'Address',
            value: entry.address,
            mono: true,
            copyable: true,
          ),

          if (entry.notes != null && entry.notes!.isNotEmpty) ...[
            const SizedBox(height: AppSpacing.md),
            _DetailItem(
              label: 'Notes',
              value: entry.notes!,
            ),
          ],

          if (entry.useCount > 0) ...[
            const SizedBox(height: AppSpacing.md),
            Row(
              children: [
                Expanded(
                  child: _DetailItem(
                    label: 'Times Used',
                    value: entry.useCount.toString(),
                  ),
                ),
                if (entry.lastUsedAt != null)
                  Expanded(
                    child: _DetailItem(
                      label: 'Last Used',
                      value: _formatDate(entry.lastUsedAt!),
                    ),
                  ),
              ],
            ),
          ],

          const SizedBox(height: AppSpacing.xl),

          PButton(
            text: 'Send to This Address',
            onPressed: onSend,
            variant: PButtonVariant.primary,
            size: PButtonSize.large,
            icon: Icon(Icons.send),
          ),

          const SizedBox(height: AppSpacing.lg),
        ],
      ),
    );
  }

  String _formatDate(DateTime date) {
    final now = DateTime.now();
    final diff = now.difference(date);

    if (diff.inDays == 0) {
      return 'Today';
    } else if (diff.inDays == 1) {
      return 'Yesterday';
    } else if (diff.inDays < 7) {
      return '${diff.inDays} days ago';
    } else {
      return '${date.month}/${date.day}/${date.year}';
    }
  }
}

/// Detail item widget
class _DetailItem extends StatelessWidget {
  final String label;
  final String value;
  final bool mono;
  final bool copyable;

  const _DetailItem({
    required this.label,
    required this.value,
    this.mono = false,
    this.copyable = false,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: AppTypography.caption.copyWith(
            color: AppColors.textSecondary,
          ),
        ),
        const SizedBox(height: 4),
        Row(
          children: [
            Expanded(
              child: Text(
                value,
                style: AppTypography.body.copyWith(
                  color: AppColors.textPrimary,
                  fontFamily: mono ? 'monospace' : null,
                  fontSize: mono ? 12 : null,
                ),
              ),
            ),
            if (copyable)
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: value));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('Copied to clipboard')),
                  );
                },
                tooltip: 'Copy',
                color: AppColors.accentPrimary,
              ),
          ],
        ),
      ],
    );
  }
}

/// Filter bottom sheet
class FilterSheet extends StatelessWidget {
  final ColorTag? currentColor;
  final bool showFavoritesOnly;
  final Map<ColorTag, int> colorCounts;
  final void Function(ColorTag?) onColorChanged;
  final VoidCallback onFavoritesToggled;
  final VoidCallback onClear;

  const FilterSheet({
    super.key,
    required this.currentColor,
    required this.showFavoritesOnly,
    required this.colorCounts,
    required this.onColorChanged,
    required this.onFavoritesToggled,
    required this.onClear,
  });

  @override
  Widget build(BuildContext context) {
    final padding = AppSpacing.screenPadding(MediaQuery.of(context).size.width);
    return Padding(
      padding: padding,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  'Filters',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: AppTypography.h3.copyWith(
                    color: AppColors.textPrimary,
                  ),
                ),
              ),
              PTextButton(
                label: 'Clear All',
                onPressed: onClear,
                variant: PTextButtonVariant.subtle,
              ),
            ],
          ),

          const SizedBox(height: AppSpacing.lg),

          // Favorites toggle
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(
              Icons.star,
              color: showFavoritesOnly ? AppColors.warning : AppColors.textTertiary,
            ),
            title: const Text('Favorites Only'),
            trailing: Switch(
              value: showFavoritesOnly,
              onChanged: (_) => onFavoritesToggled(),
              activeThumbColor: AppColors.accentPrimary,
            ),
            onTap: onFavoritesToggled,
          ),

          const Divider(),

          const SizedBox(height: AppSpacing.sm),

          Text(
            'Filter by Color',
            style: AppTypography.labelMedium.copyWith(
              color: AppColors.textSecondary,
            ),
          ),

          const SizedBox(height: AppSpacing.md),

          Wrap(
            spacing: AppSpacing.sm,
            runSpacing: AppSpacing.sm,
            children: ColorTag.values.map((color) {
              final count = colorCounts[color] ?? 0;
              final isSelected = currentColor == color;

              if (color == ColorTag.none && count == 0) {
                return const SizedBox.shrink();
              }

              return FilterChip(
                label: Text('${color.displayName} ($count)'),
                selected: isSelected,
                onSelected: (_) => onColorChanged(isSelected ? null : color),
                backgroundColor: color.color.withValues(alpha: 0.2),
                selectedColor: color.color.withValues(alpha: 0.4),
                checkmarkColor: color.color,
                labelStyle: TextStyle(
                  color: isSelected ? Colors.white : color.color,
                ),
              );
            }).toList(),
          ),

          const SizedBox(height: AppSpacing.lg),
        ],
      ),
    );
  }
}

class _AddressQrScannerScreen extends StatefulWidget {
  const _AddressQrScannerScreen();

  @override
  State<_AddressQrScannerScreen> createState() => _AddressQrScannerScreenState();
}

class _AddressQrScannerScreenState extends State<_AddressQrScannerScreen> {
  final mobile_scanner.MobileScannerController _controller = mobile_scanner.MobileScannerController(
    detectionSpeed: mobile_scanner.DetectionSpeed.noDuplicates,
    formats: [mobile_scanner.BarcodeFormat.qrCode],
  );
  bool _handled = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return PScaffold(
      title: 'Scan QR',
      appBar: const PAppBar(
        title: 'Scan QR',
        subtitle: 'Align the code in the frame',
        showBackButton: true,
      ),
      body: ColoredBox(
        color: Colors.black,
        child: mobile_scanner.MobileScanner(
          controller: _controller,
          onDetect: (capture) {
            if (_handled) return;
            final barcodes = capture.barcodes;
            for (final barcode in barcodes) {
              final value = barcode.rawValue;
              if (value != null && value.isNotEmpty) {
                _handled = true;
                Navigator.of(context).pop(value);
                break;
              }
            }
          },
        ),
      ),
    );
  }
}
