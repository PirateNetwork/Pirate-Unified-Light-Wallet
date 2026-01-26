/// Diagnostics Screen — Sync logs and recovery actions
///
/// Features:
/// - Last 200 sync logs (local only, never sent)
/// - Copy redacted logs (opt-in)
/// - Rebuild from last checkpoint action
/// - Real-time sync status during rescan
library;

import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../../design/deep_space_theme.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/ffi/generated/models.dart' show SyncStatus;
import '../../../core/ffi/generated/api.dart' as api;
import '../../../core/providers/wallet_providers.dart';
import '../../../ui/atoms/p_icon_button.dart';
import '../../../ui/atoms/p_text_button.dart';
import '../../../ui/organisms/p_app_bar.dart';
import '../../../ui/organisms/p_scaffold.dart';

/// Diagnostics screen with real FFI integration
class DiagnosticsScreen extends ConsumerStatefulWidget {
  const DiagnosticsScreen({super.key});

  @override
  ConsumerState<DiagnosticsScreen> createState() => _DiagnosticsScreenState();
}

class _DiagnosticsScreenState extends ConsumerState<DiagnosticsScreen> {
  bool _isRebuilding = false;
  SyncLogLevel? _filterLevel;
  String _searchQuery = '';
  final TextEditingController _searchController = TextEditingController();
  final ScrollController _scrollController = ScrollController();
  StreamSubscription<SyncStatus>? _syncSubscription;
  SyncStatus? _rescanStatus;

  @override
  void dispose() {
    _searchController.dispose();
    _scrollController.dispose();
    _syncSubscription?.cancel();
    super.dispose();
  }

  List<SyncLogEntryFfi> _filterLogs(List<SyncLogEntryFfi> logs) {
    return logs.where((log) {
      if (_filterLevel != null && log.level != _filterLevel) {
        return false;
      }
      if (_searchQuery.isNotEmpty) {
        final query = _searchQuery.toLowerCase();
        return log.message.toLowerCase().contains(query) ||
            log.module.toLowerCase().contains(query);
      }
      return true;
    }).toList();
  }

  Future<void> _copyRedactedLogs(List<SyncLogEntryFfi> logs) async {
    final filteredLogs = _filterLogs(logs);
    
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.surfaceElevated,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
        title: Text(
          'Copy Redacted Logs?',
          style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'The following data will be automatically redacted:',
              style: AppTypography.body.copyWith(color: AppColors.textSecondary),
            ),
            const SizedBox(height: AppSpacing.sm),
            _RedactionItem(label: 'Addresses', example: 'zs1abc... → [REDACTED_ADDRESS]'),
            _RedactionItem(label: 'Hashes', example: '0xabc... → [REDACTED_HASH]'),
            _RedactionItem(label: 'IP addresses', example: '192.168.1.1 → [REDACTED_IP]'),
            _RedactionItem(label: 'Emails', example: 'user@... → [REDACTED_EMAIL]'),
            const SizedBox(height: AppSpacing.md),
            Container(
              padding: const EdgeInsets.all(AppSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.warning.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: AppColors.warning.withValues(alpha: 0.3),
                ),
              ),
              child: Row(
                children: [
                  Icon(Icons.info_outline, color: AppColors.warning, size: 16),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Text(
                      'Logs are never sent automatically. You control what you share.',
                      style: AppTypography.caption.copyWith(
                        color: AppColors.warning,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
        actions: [
          PTextButton(
            label: 'Cancel',
            onPressed: () => Navigator.of(context).pop(false),
            variant: PTextButtonVariant.subtle,
          ),
          PTextButton(
            label: 'Copy',
            onPressed: () => Navigator.of(context).pop(true),
          ),
        ],
      ),
    );

    if (confirmed ?? false) {
      final redactedLogs = filteredLogs.map((l) => l.toRedactedString()).join('\n');

      await Clipboard.setData(ClipboardData(text: redactedLogs));
      DeepSpaceHaptics.lightImpact();

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              'Copied ${filteredLogs.length} redacted log entries',
              style: AppTypography.body.copyWith(color: AppColors.textPrimary),
            ),
            backgroundColor: AppColors.surfaceElevated,
            action: SnackBarAction(
              label: 'Done',
              textColor: AppColors.gradientAStart,
              onPressed: () {},
            ),
          ),
        );
      }
    }
  }

  Future<void> _rebuildFromCheckpoint() async {
    // Get checkpoint from provider
    final checkpointAsync = ref.read(lastCheckpointProvider);
    final checkpoint = checkpointAsync.value;

    if (checkpoint == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            'No checkpoint available',
            style: AppTypography.body.copyWith(color: AppColors.textPrimary),
          ),
          backgroundColor: AppColors.error,
        ),
      );
      return;
    }

    final checkpointHeight = checkpoint.height;

    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: AppColors.surfaceElevated,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
        title: Text(
          'Rescan from Checkpoint?',
          style: AppTypography.h3.copyWith(color: AppColors.textPrimary),
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'This will:',
              style: AppTypography.body.copyWith(color: AppColors.textSecondary),
            ),
            const SizedBox(height: AppSpacing.sm),
              _ActionItem(text: 'Restore wallet state to height ${_formatHeight(checkpointHeight)}'),
            _ActionItem(text: 'Remove transactions after this height'),
            _ActionItem(text: 'Re-scan blocks from checkpoint'),
            const SizedBox(height: AppSpacing.md),
            Container(
              padding: const EdgeInsets.all(AppSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.surfaceElevated,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: AppColors.borderSubtle),
              ),
              child: Row(
                children: [
                  Icon(Icons.access_time, color: AppColors.textSecondary, size: 16),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Text(
                      'Checkpoint from ${_formatCheckpointTime(DateTime.fromMillisecondsSinceEpoch(checkpoint.timestamp))}',
                      style: AppTypography.caption.copyWith(
                        color: AppColors.textSecondary,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: AppSpacing.sm),
            Container(
              padding: const EdgeInsets.all(AppSpacing.sm),
              decoration: BoxDecoration(
                color: AppColors.warning.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                  color: AppColors.warning.withValues(alpha: 0.3),
                ),
              ),
              child: Row(
                children: [
                  Icon(Icons.warning_amber, color: AppColors.warning, size: 16),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: Text(
                      'Use this if sync appears stuck or corrupted. Your funds are safe.',
                      style: AppTypography.caption.copyWith(
                        color: AppColors.warning,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
        actions: [
          PTextButton(
            label: 'Cancel',
            onPressed: () => Navigator.of(context).pop(false),
            variant: PTextButtonVariant.subtle,
          ),
          PTextButton(
            label: 'Rescan',
            onPressed: () => Navigator.of(context).pop(true),
            variant: PTextButtonVariant.danger,
          ),
        ],
      ),
    );

    if (confirmed ?? false) {
      setState(() => _isRebuilding = true);

      try {
        // Start listening to sync progress
        final walletId = ref.read(activeWalletProvider);
        if (walletId != null) {
          await _syncSubscription?.cancel();
          _syncSubscription = FfiBridge.syncProgressStream(walletId).listen(
            (status) {
              if (mounted) {
                setState(() => _rescanStatus = status);
              }
            },
            onError: (e) {
              if (mounted) {
                setState(() {
                  _isRebuilding = false;
                  _rescanStatus = null;
                });
              }
            },
          );
        }

        // Call real FFI rescan
        final rescan = ref.read(rescanProvider);
        await rescan(checkpointHeight);

        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                'Rescan started from height ${_formatHeight(checkpointHeight)}',
                style: AppTypography.body.copyWith(color: AppColors.textPrimary),
              ),
              backgroundColor: AppColors.surfaceElevated,
            ),
          );

          // Refresh logs to show new entries
          ref.read(refreshSyncLogsProvider)();
        }
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                'Rescan failed: $e',
                style: AppTypography.body.copyWith(color: AppColors.textPrimary),
              ),
              backgroundColor: AppColors.error,
            ),
          );
          setState(() {
            _isRebuilding = false;
            _rescanStatus = null;
          });
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final logsAsync = ref.watch(syncLogsProvider);

    return PScaffold(
      title: 'Diagnostics',
      appBar: PAppBar(
        title: 'Diagnostics',
        subtitle: 'Sync health & logs',
        actions: [
          PIconButton(
            icon: Icon(Icons.refresh, color: AppColors.textSecondary),
            onPressed: () {
              ref.read(refreshSyncLogsProvider)();
              ref.invalidate(lastCheckpointProvider);
            },
            tooltip: 'Refresh logs',
          ),
        ],
      ),
      body: Column(
        children: [
          // Rescan progress (if active)
          if (_isRebuilding && _rescanStatus != null) _buildRescanProgress(),

          // Checkpoint info card
          _buildCheckpointCard(),

          // Search and filter bar
          _buildSearchBar(),

          // Log level filter chips
          _buildFilterChips(),

          // Logs list
          Expanded(
            child: logsAsync.when(
              data: (logs) => _buildLogsList(_filterLogs(logs)),
              loading: () => Center(
                child: CircularProgressIndicator(
                  color: AppColors.gradientAStart,
                ),
              ),
              error: (e, _) => Center(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.error_outline, size: 48, color: AppColors.error),
                    const SizedBox(height: AppSpacing.md),
                    Text(
                      'Failed to load logs',
                      style: AppTypography.body.copyWith(color: AppColors.textMuted),
                    ),
                    const SizedBox(height: AppSpacing.sm),
                    PTextButton(
                      label: 'Retry',
                      onPressed: () => ref.read(refreshSyncLogsProvider)(),
                    ),
                  ],
                ),
              ),
            ),
          ),

          // Action buttons
          logsAsync.when(
            data: _buildActions,
            loading: () => _buildActions([]),
            error: (_, _) => _buildActions([]),
          ),
        ],
      ),
    );
  }

  Widget _buildRescanProgress() {
    final status = _rescanStatus!;
    final isComplete = status.percent >= 100;

    if (isComplete) {
      // Auto-dismiss after completion
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted) {
          setState(() {
            _isRebuilding = false;
            _rescanStatus = null;
          });
          _syncSubscription?.cancel();
        }
      });
    }

    return Container(
      margin: const EdgeInsets.all(AppSpacing.md),
      padding: const EdgeInsets.all(AppSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.gradientAStart.withValues(alpha: 0.1),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: AppColors.gradientAStart.withValues(alpha: 0.3),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              if (!isComplete)
                SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: AppColors.gradientAStart,
                  ),
                )
              else
                Icon(Icons.check_circle, size: 16, color: AppColors.success),
              const SizedBox(width: AppSpacing.sm),
              Text(
                isComplete ? 'Rescan Complete' : 'Rescanning...',
                style: AppTypography.body.copyWith(
                  color: AppColors.textPrimary,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const Spacer(),
              Text(
                '${status.percent.toStringAsFixed(1)}%',
                style: AppTypography.body.copyWith(
                  color: AppColors.gradientAStart,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
          const SizedBox(height: AppSpacing.sm),
          ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: LinearProgressIndicator(
              value: status.percent / 100,
              backgroundColor: AppColors.surfaceElevated,
              valueColor: AlwaysStoppedAnimation<Color>(AppColors.gradientAStart),
            ),
          ),
          const SizedBox(height: AppSpacing.sm),
          Row(
            children: [
              Expanded(
                child: Text(
                  'Height: ${_formatHeight(status.localHeight.toInt())}',
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: AppTypography.caption
                      .copyWith(color: AppColors.textSecondary),
                ),
              ),
              if (status.eta != null && !isComplete)
                Text(
                  'ETA: ${status.etaFormatted}',
                  style: AppTypography.caption
                      .copyWith(color: AppColors.textSecondary),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildCheckpointCard() {
    final checkpointAsync = ref.watch(lastCheckpointProvider);

    return checkpointAsync.when(
      data: _buildCheckpointCardContent,
      loading: () => _buildCheckpointCardContent(null, isLoading: true),
      error: (_, _) => _buildCheckpointCardContent(null),
    );
  }

  Widget _buildCheckpointCardContent(api.CheckpointInfo? checkpoint, {bool isLoading = false}) {
    return Container(
      margin: const EdgeInsets.all(AppSpacing.md),
      padding: const EdgeInsets.all(AppSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.surfaceElevated,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppColors.borderSubtle),
      ),
      child: Row(
        children: [
          Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: AppColors.gradientAStart.withValues(alpha: 0.15),
              shape: BoxShape.circle,
            ),
            child: isLoading
                ? Center(
                    child: SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: AppColors.gradientAStart,
                      ),
                    ),
                  )
                : Icon(
                    Icons.save_outlined,
                    color: AppColors.gradientAStart,
                    size: 20,
                  ),
          ),
          const SizedBox(width: AppSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Last Checkpoint',
                  style: AppTypography.caption.copyWith(
                    color: AppColors.textMuted,
                  ),
                ),
                Text(
                  checkpoint != null
                      ? 'Height ${_formatHeight(checkpoint.height)}'
                      : 'No checkpoint',
                  style: AppTypography.body.copyWith(
                    color: AppColors.textPrimary,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (checkpoint != null)
                  Text(
                    _formatCheckpointTime(DateTime.fromMillisecondsSinceEpoch(checkpoint.timestamp)),
                    style: AppTypography.caption.copyWith(
                      color: AppColors.textSecondary,
                    ),
                  ),
              ],
            ),
          ),
          if (checkpoint != null)
            Container(
              padding: const EdgeInsets.symmetric(
                horizontal: AppSpacing.sm,
                vertical: AppSpacing.xs,
              ),
              decoration: BoxDecoration(
                color: AppColors.success.withValues(alpha: 0.15),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Text(
                'Valid',
                style: AppTypography.caption.copyWith(
                  color: AppColors.success,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildSearchBar() {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppSpacing.md),
      child: TextField(
        controller: _searchController,
        style: AppTypography.body.copyWith(color: AppColors.textPrimary),
        onChanged: (v) => setState(() => _searchQuery = v),
        decoration: InputDecoration(
          hintText: 'Search logs...',
          hintStyle: AppTypography.body.copyWith(color: AppColors.textMuted),
          prefixIcon: Icon(Icons.search, color: AppColors.textMuted),
          suffixIcon: _searchQuery.isNotEmpty
              ? IconButton(
                  icon: Icon(Icons.clear, color: AppColors.textMuted),
                  onPressed: () {
                    _searchController.clear();
                    setState(() => _searchQuery = '');
                  },
                )
              : null,
          filled: true,
          fillColor: AppColors.surfaceElevated,
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(12),
            borderSide: BorderSide.none,
          ),
          contentPadding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.md,
            vertical: AppSpacing.sm,
          ),
        ),
      ),
    );
  }

  Widget _buildFilterChips() {
    return Container(
      height: 48,
      margin: const EdgeInsets.symmetric(vertical: AppSpacing.sm),
      child: ListView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: AppSpacing.md),
        children: [
          _FilterChip(
            label: 'All',
            isSelected: _filterLevel == null,
            onTap: () => setState(() => _filterLevel = null),
          ),
          ...SyncLogLevel.values.map((level) => _FilterChip(
                label: level.label,
                color: _getLogLevelColor(level),
                isSelected: _filterLevel == level,
                onTap: () => setState(() => _filterLevel = level),
              )),
        ],
      ),
    );
  }

  Color _getLogLevelColor(SyncLogLevel level) {
    switch (level) {
      case SyncLogLevel.debug:
        return const Color(0xFF6B7280);
      case SyncLogLevel.info:
        return const Color(0xFF3B82F6);
      case SyncLogLevel.warn:
        return const Color(0xFFF59E0B);
      case SyncLogLevel.error:
        return const Color(0xFFEF4444);
    }
  }

  Widget _buildLogsList(List<SyncLogEntryFfi> logs) {
    if (logs.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.article_outlined,
              size: 48,
              color: AppColors.textMuted,
            ),
            const SizedBox(height: AppSpacing.md),
            Text(
              'No logs found',
              style: AppTypography.body.copyWith(
                color: AppColors.textMuted,
              ),
            ),
          ],
        ),
      );
    }

    return ListView.builder(
      controller: _scrollController,
      padding: const EdgeInsets.symmetric(horizontal: AppSpacing.md),
      itemCount: logs.length,
      itemBuilder: (context, index) {
        final log = logs[index];
        return _LogEntryTile(log: log);
      },
    );
  }

  Widget _buildActions(List<SyncLogEntryFfi> logs) {
    final checkpointAsync = ref.watch(lastCheckpointProvider);
    final hasCheckpoint = checkpointAsync.value != null;

    return Container(
      padding: const EdgeInsets.all(AppSpacing.md),
      decoration: BoxDecoration(
        color: AppColors.voidBlack,
        border: Border(
          top: BorderSide(color: AppColors.borderSubtle),
        ),
      ),
      child: SafeArea(
        top: false,
        child: Row(
          children: [
            Expanded(
              child: OutlinedButton.icon(
                onPressed: logs.isEmpty ? null : () => _copyRedactedLogs(logs),
                icon: Icon(Icons.copy, size: 18),
                label: const Text('Copy Redacted Logs'),
                style: OutlinedButton.styleFrom(
                  foregroundColor: AppColors.textSecondary,
                  side: BorderSide(color: AppColors.borderSubtle),
                  padding: const EdgeInsets.symmetric(vertical: 14),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                  ),
                ),
              ),
            ),
            const SizedBox(width: AppSpacing.md),
            Expanded(
              child: _isRebuilding
                  ? Container(
                      height: 48,
                      decoration: BoxDecoration(
                        color: AppColors.warning.withValues(alpha: 0.2),
                        borderRadius: BorderRadius.circular(12),
                      ),
                      child: Center(
                        child: SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: AppColors.warning,
                          ),
                        ),
                      ),
                    )
                  : OutlinedButton.icon(
                      onPressed: hasCheckpoint ? _rebuildFromCheckpoint : null,
                      icon: Icon(Icons.replay, size: 18),
                      label: const Text('Rescan'),
                      style: OutlinedButton.styleFrom(
                        foregroundColor: hasCheckpoint
                            ? AppColors.warning
                            : AppColors.textMuted,
                        side: BorderSide(
                          color: hasCheckpoint
                              ? AppColors.warning.withValues(alpha: 0.5)
                              : AppColors.borderSubtle,
                        ),
                        padding: const EdgeInsets.symmetric(vertical: 14),
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(12),
                        ),
                      ),
                    ),
            ),
          ],
        ),
      ),
    );
  }

  String _formatHeight(int height) {
    final text = height.toString();
    return text.replaceAllMapped(
      RegExp(r'(\d{1,3})(?=(\d{3})+(?!\d))'),
      (m) => '${m[1]},',
    );
  }

  String _formatCheckpointTime(DateTime time) {
    final diff = DateTime.now().difference(time);
    if (diff.inSeconds < 60) return '${diff.inSeconds}s ago';
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    if (diff.inHours < 24) return '${diff.inHours}h ago';
    return '${diff.inDays}d ago';
  }
}

// =============================================================================
// Supporting Widgets
// =============================================================================

class _FilterChip extends StatelessWidget {
  const _FilterChip({
    required this.label,
    required this.isSelected,
    required this.onTap,
    this.color,
  });

  final String label;
  final bool isSelected;
  final VoidCallback onTap;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(right: AppSpacing.sm),
      child: GestureDetector(
        onTap: onTap,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 200),
          padding: const EdgeInsets.symmetric(
            horizontal: AppSpacing.md,
            vertical: AppSpacing.sm,
          ),
          decoration: BoxDecoration(
            color: isSelected
                ? (color ?? AppColors.gradientAStart)
                    .withValues(alpha: 0.2)
                : AppColors.surfaceElevated,
            borderRadius: BorderRadius.circular(20),
            border: Border.all(
              color: isSelected
                  ? (color ?? AppColors.gradientAStart)
                  : AppColors.borderSubtle,
              width: isSelected ? 2 : 1,
            ),
          ),
          child: Text(
            label,
            style: AppTypography.caption.copyWith(
              color: isSelected
                  ? (color ?? AppColors.gradientAStart)
                  : AppColors.textSecondary,
              fontWeight: isSelected ? FontWeight.w600 : FontWeight.normal,
            ),
          ),
        ),
      ),
    );
  }
}

class _LogEntryTile extends StatelessWidget {
  const _LogEntryTile({required this.log});

  final SyncLogEntryFfi log;

  Color _getLogLevelColor(SyncLogLevel level) {
    switch (level) {
      case SyncLogLevel.debug:
        return const Color(0xFF6B7280);
      case SyncLogLevel.info:
        return const Color(0xFF3B82F6);
      case SyncLogLevel.warn:
        return const Color(0xFFF59E0B);
      case SyncLogLevel.error:
        return const Color(0xFFEF4444);
    }
  }

  String _formatTime(DateTime t) {
    return '${t.hour.toString().padLeft(2, '0')}:'
        '${t.minute.toString().padLeft(2, '0')}:'
        '${t.second.toString().padLeft(2, '0')}.'
        '${t.millisecond.toString().padLeft(3, '0')}';
  }

  @override
  Widget build(BuildContext context) {
    final color = _getLogLevelColor(log.level);

    return Container(
      margin: const EdgeInsets.only(bottom: 4),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpacing.sm,
        vertical: AppSpacing.xs,
      ),
      decoration: BoxDecoration(
        color: log.level == SyncLogLevel.error
            ? AppColors.error.withValues(alpha: 0.05)
            : log.level == SyncLogLevel.warn
                ? AppColors.warning.withValues(alpha: 0.05)
                : Colors.transparent,
        borderRadius: BorderRadius.circular(4),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Timestamp
          Text(
            _formatTime(log.timestamp),
            style: AppTypography.code.copyWith(
              color: AppColors.textMuted,
              fontSize: 10,
            ),
          ),
          const SizedBox(width: 8),

          // Level badge
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
            decoration: BoxDecoration(
              color: color.withValues(alpha: 0.2),
              borderRadius: BorderRadius.circular(2),
            ),
            child: Text(
              log.level.label,
              style: AppTypography.code.copyWith(
                color: color,
                fontSize: 9,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          const SizedBox(width: 8),

          // Module
          Flexible(
            child: Text(
              '[${log.module}]',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: AppTypography.code.copyWith(
                color: AppColors.gradientAStart,
                fontSize: 10,
              ),
            ),
          ),
          const SizedBox(width: 8),

          // Message
          Expanded(
            child: Text(
              log.message,
              style: AppTypography.code.copyWith(
                color: AppColors.textSecondary,
                fontSize: 10,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _RedactionItem extends StatelessWidget {
  const _RedactionItem({required this.label, required this.example});

  final String label;
  final String example;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          Icon(
            Icons.check,
            size: 14,
            color: AppColors.success,
          ),
          const SizedBox(width: 8),
          Text(
            '$label: ',
            style: AppTypography.caption.copyWith(
              color: AppColors.textPrimary,
              fontWeight: FontWeight.w600,
            ),
          ),
          Expanded(
            child: Text(
              example,
              style: AppTypography.code.copyWith(
                color: AppColors.textMuted,
                fontSize: 10,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ActionItem extends StatelessWidget {
  const _ActionItem({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '•',
            style: AppTypography.body.copyWith(
              color: AppColors.textSecondary,
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              text,
              style: AppTypography.body.copyWith(
                color: AppColors.textSecondary,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
