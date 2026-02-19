/// Address Book List Screen
///
/// Displays all saved addresses with color tags, labels, and quick actions.
/// Features:
/// - Search and filter
/// - Color-coded entries
/// - Swipe actions (edit, delete)
/// - Empty state illustration
/// - WCAG AA compliant
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../design/deep_space_theme.dart';
import '../../ui/atoms/p_button.dart';
import '../../ui/atoms/p_gradient_button.dart';
import '../../ui/molecules/p_dialog.dart';
import '../../ui/molecules/wallet_switcher.dart';
import '../../core/providers/wallet_providers.dart';
import 'address_book_detail_screen.dart';
import 'models/address_entry.dart';
import 'providers/address_book_provider.dart';
import '../../core/i18n/arb_text_localizer.dart';

/// Address Book List Screen
class AddressBookListScreen extends ConsumerStatefulWidget {
  const AddressBookListScreen({super.key});

  @override
  ConsumerState<AddressBookListScreen> createState() =>
      _AddressBookListScreenState();
}

class _AddressBookListScreenState extends ConsumerState<AddressBookListScreen> {
  final TextEditingController _searchController = TextEditingController();
  final FocusNode _searchFocusNode = FocusNode();

  ColorTag? _filterColor;

  @override
  void initState() {
    super.initState();
    _searchController.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _searchController.dispose();
    _searchFocusNode.dispose();
    super.dispose();
  }

  List<AddressEntry> _applyFilter(List<AddressEntry> entries) {
    final query = _searchController.text.toLowerCase();
    return entries.where((entry) {
      final matchesQuery =
          query.isEmpty ||
          entry.label.toLowerCase().contains(query) ||
          entry.address.toLowerCase().contains(query) ||
          (entry.notes?.toLowerCase().contains(query) ?? false);
      final matchesColor =
          _filterColor == null || entry.colorTag == _filterColor;
      return matchesQuery && matchesColor;
    }).toList();
  }

  void _setFilterColor(ColorTag? color) {
    setState(() {
      _filterColor = color;
    });
  }

  void _navigateToDetail(String walletId, AddressEntry entry) {
    Navigator.of(context)
        .push<bool?>(
          MaterialPageRoute<bool?>(
            builder: (_) => AddressBookDetailScreen(entry: entry),
          ),
        )
        .then((result) {
          if (result ?? false) {
            _refreshEntries(walletId);
          }
        });
  }

  void _navigateToAdd(String walletId) {
    Navigator.of(context)
        .push<void>(
          MaterialPageRoute<void>(
            builder: (_) => AddressBookEditScreen(walletId: walletId),
          ),
        )
        .then((_) => _refreshEntries(walletId));
  }

  Future<void> _refreshEntries(String walletId) {
    return ref.read(addressBookProvider(walletId).notifier).refresh();
  }

  Future<void> _deleteEntry(AddressEntry entry) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => _DeleteConfirmationDialog(entry: entry),
    );

    if (confirmed ?? false) {
      final walletId = entry.walletId;
      final success = await ref
          .read(addressBookProvider(walletId).notifier)
          .deleteEntry(entry.id);
      if (mounted && success) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Deleted "${entry.label}"'),
            backgroundColor: AppColors.surfaceElevated,
          ),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final walletId = ref.watch(activeWalletProvider);
    if (walletId == null) {
      return Scaffold(
        backgroundColor: AppColors.deepSpace,
        body: SafeArea(child: Center(child: Text('No active wallet'.tr))),
      );
    }

    final state = ref.watch(addressBookProvider(walletId));
    final filteredEntries = _applyFilter(state.entries);

    return Scaffold(
      backgroundColor: AppColors.deepSpace,
      body: SafeArea(
        child: Column(
          children: [
            // Hero Header
            PGradientHeroHeader(
              title: 'Address Book'.tr,
              subtitle: '${state.entries.length} saved addresses',
              actions: [
                const WalletSwitcherButton(compact: true),
                PGradientButton(
                  text: 'Add',
                  icon: Icons.add,
                  size: PGradientButtonSize.small,
                  onPressed: () => _navigateToAdd(walletId),
                ),
              ],
            ),

            // Search and Filter Bar
            _buildSearchBar(),

            // Color Filter Chips
            _buildColorFilters(),

            // List
            Expanded(child: _buildContent(state, filteredEntries, walletId)),
          ],
        ),
      ),
    );
  }

  Widget _buildSearchBar() {
    return Container(
      padding: const EdgeInsets.all(AppSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.voidBlack,
        border: Border(
          bottom: BorderSide(color: AppColors.borderSubtle, width: 1.0),
        ),
      ),
      child: TextField(
        controller: _searchController,
        focusNode: _searchFocusNode,
        style: AppTypography.body.copyWith(color: AppColors.textPrimary),
        decoration: InputDecoration(
          hintText: 'Search name, address, or notes'.tr,
          hintStyle: AppTypography.body.copyWith(color: AppColors.textMuted),
          prefixIcon: Icon(Icons.search, color: AppColors.textMuted),
          suffixIcon: _searchController.text.isNotEmpty
              ? IconButton(
                  icon: Icon(Icons.clear, color: AppColors.textMuted),
                  onPressed: _searchController.clear,
                )
              : null,
          filled: true,
          fillColor: AppColors.surfaceElevated,
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(12.0),
            borderSide: BorderSide.none,
          ),
          focusedBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(12.0),
            borderSide: BorderSide(color: AppColors.gradientAStart, width: 2.0),
          ),
          contentPadding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.md,
            vertical: AppSpacing.sm,
          ),
        ),
      ),
    );
  }

  Widget _buildColorFilters() {
    return Container(
      height: 56,
      padding: const EdgeInsets.symmetric(horizontal: AppSpacing.md),
      child: ListView(
        scrollDirection: Axis.horizontal,
        children: [
          _ColorFilterChip(
            label: 'All'.tr,
            color: null,
            isSelected: _filterColor == null,
            onSelected: () => _setFilterColor(null),
          ),
          ...ColorTag.values
              .where((c) => c != ColorTag.none)
              .map(
                (color) => _ColorFilterChip(
                  label: color.displayName,
                  color: color,
                  isSelected: _filterColor == color,
                  onSelected: () => _setFilterColor(color),
                ),
              ),
        ],
      ),
    );
  }

  Widget _buildContent(
    AddressBookState state,
    List<AddressEntry> filteredEntries,
    String walletId,
  ) {
    if (state.isLoading) {
      return Center(
        child: CircularProgressIndicator(color: AppColors.gradientAStart),
      );
    }

    if (state.error != null) {
      return _buildErrorState(state.error ?? 'Unknown error', walletId);
    }

    if (filteredEntries.isEmpty) {
      return _buildEmptyState(walletId);
    }

    return RefreshIndicator(
      onRefresh: () => _refreshEntries(walletId),
      color: AppColors.gradientAStart,
      backgroundColor: AppColors.surfaceElevated,
      child: ListView.builder(
        padding: const EdgeInsets.all(AppSpacing.md),
        itemCount: filteredEntries.length,
        itemBuilder: (context, index) {
          final entry = filteredEntries[index];
          return Padding(
            padding: const EdgeInsets.only(bottom: AppSpacing.sm),
            child: _AddressBookEntryCard(
              entry: entry,
              onTap: () => _navigateToDetail(walletId, entry),
              onDelete: () => _deleteEntry(entry),
            ),
          );
        },
      ),
    );
  }

  Widget _buildEmptyState(String walletId) {
    final hasSearch = _searchController.text.isNotEmpty || _filterColor != null;

    return Center(
      child: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            // Empty state icon with gradient
            Container(
              width: 80,
              height: 80,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: AppColors.gradientALinear,
                boxShadow: [
                  BoxShadow(
                    color: AppColors.glow.withValues(alpha: 0.3),
                    blurRadius: 20,
                    spreadRadius: 2,
                  ),
                ],
              ),
              child: Icon(
                hasSearch ? Icons.search_off : Icons.contacts_outlined,
                size: 40,
                color: AppColors.textOnGradient,
              ),
            ),
            const SizedBox(height: AppSpacing.lg),
            Text(
              hasSearch ? 'No matches found' : 'No saved addresses',
              style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
            ),
            const SizedBox(height: AppSpacing.sm),
            Text(
              hasSearch
                  ? 'Try adjusting your search or filters'
                  : 'Save trusted addresses for faster sends.',
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            if (!hasSearch) ...[
              const SizedBox(height: AppSpacing.xl),
              PGradientButton(
                text: 'Add First Address',
                icon: Icons.add,
                onPressed: () => _navigateToAdd(walletId),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildErrorState(String error, String walletId) {
    return Center(
      child: Padding(
        padding: AppSpacing.screenPadding(
          MediaQuery.of(context).size.width,
          vertical: AppSpacing.xl,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.error_outline, size: 64, color: AppColors.error),
            const SizedBox(height: AppSpacing.lg),
            Text(
              'Failed to load addresses'.tr,
              style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
            ),
            const SizedBox(height: AppSpacing.sm),
            Text(
              error,
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: AppSpacing.xl),
            PGradientButton(
              text: 'Retry',
              icon: Icons.refresh,
              onPressed: () => _refreshEntries(walletId),
            ),
          ],
        ),
      ),
    );
  }
}

// =============================================================================
// Supporting Widgets
// =============================================================================

class _ColorFilterChip extends StatelessWidget {
  const _ColorFilterChip({
    required this.label,
    required this.color,
    required this.isSelected,
    required this.onSelected,
  });

  final String label;
  final ColorTag? color;
  final bool isSelected;
  final VoidCallback onSelected;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(right: AppSpacing.sm),
      child: GestureDetector(
        onTap: onSelected,
        child: AnimatedContainer(
          duration: DeepSpaceDurations.fast,
          padding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.md,
            vertical: AppSpacing.sm,
          ),
          decoration: BoxDecoration(
            color: isSelected
                ? (color?.color ?? AppColors.gradientAStart).withValues(
                    alpha: 0.2,
                  )
                : AppColors.surfaceElevated,
            borderRadius: BorderRadius.circular(20),
            border: Border.all(
              color: isSelected
                  ? (color?.color ?? AppColors.gradientAStart)
                  : AppColors.borderSubtle,
              width: isSelected ? 2 : 1,
            ),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (color != null) ...[
                Container(
                  width: 12,
                  height: 12,
                  decoration: BoxDecoration(
                    color: color!.color,
                    shape: BoxShape.circle,
                  ),
                ),
                const SizedBox(width: AppSpacing.xs),
              ],
              Text(
                label,
                style: AppTypography.caption.copyWith(
                  color: isSelected
                      ? (color?.color ?? AppColors.gradientAStart)
                      : AppColors.textSecondary,
                  fontWeight: isSelected ? FontWeight.w600 : FontWeight.normal,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _AddressBookEntryCard extends StatelessWidget {
  const _AddressBookEntryCard({
    required this.entry,
    required this.onTap,
    required this.onDelete,
  });

  final AddressEntry entry;
  final VoidCallback onTap;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    return Dismissible(
      key: ValueKey(entry.id),
      direction: DismissDirection.endToStart,
      background: Container(
        alignment: Alignment.centerRight,
        padding: const EdgeInsets.only(right: AppSpacing.lg),
        decoration: BoxDecoration(
          color: AppColors.error.withValues(alpha: 0.8),
          borderRadius: BorderRadius.circular(16),
        ),
        child: Icon(Icons.delete_outline, color: Colors.white, size: 28),
      ),
      confirmDismiss: (_) async {
        onDelete();
        return false; // We handle deletion in the callback
      },
      child: Semantics(
        label: '${entry.label}, ${entry.truncatedAddress}',
        button: true,
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: onTap,
            borderRadius: BorderRadius.circular(16),
            child: Container(
              padding: const EdgeInsets.all(AppSpacing.md),
              decoration: BoxDecoration(
                color: AppColors.surfaceElevated,
                borderRadius: BorderRadius.circular(16),
                border: Border.all(color: AppColors.borderSubtle, width: 1),
              ),
              child: Row(
                children: [
                  // Color indicator
                  Container(
                    width: 4,
                    height: 48,
                    decoration: BoxDecoration(
                      color: entry.colorTag.color,
                      borderRadius: BorderRadius.circular(2),
                    ),
                  ),
                  const SizedBox(width: AppSpacing.md),

                  // Content
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          entry.label,
                          style: AppTypography.body.copyWith(
                            color: AppColors.textPrimary,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          entry.truncatedAddress,
                          style: AppTypography.code.copyWith(
                            color: AppColors.textMuted,
                            fontSize: 12,
                          ),
                        ),
                        if (entry.notes != null && entry.notes!.isNotEmpty) ...[
                          const SizedBox(height: 4),
                          Text(
                            entry.notes!,
                            style: AppTypography.caption.copyWith(
                              color: AppColors.textSecondary,
                            ),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ],
                      ],
                    ),
                  ),

                  // Chevron
                  Icon(
                    Icons.chevron_right,
                    color: AppColors.textMuted,
                    size: 24,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _DeleteConfirmationDialog extends StatelessWidget {
  const _DeleteConfirmationDialog({required this.entry});

  final AddressEntry entry;

  @override
  Widget build(BuildContext context) {
    return PDialog(
      title: 'Delete address?'.tr,
      content: Text(
        'Remove "${entry.label}" from your address book? This cannot be undone.',
        style: AppTypography.body.copyWith(color: AppColors.textSecondary),
      ),
      actions: [
        PDialogAction(
          label: 'Cancel'.tr,
          variant: PButtonVariant.outline,
          result: false,
        ),
        PDialogAction(
          label: 'Delete'.tr,
          variant: PButtonVariant.danger,
          result: true,
        ),
      ],
    );
  }
}
