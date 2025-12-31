/// Sync Indicator — Real-time sync progress with particle animation
///
/// Shows sync stage, heights, percentage, ETA, and last checkpoint.
/// Features particle/blur animation tied to blocks/second performance.
library;

import 'dart:math' as math;
import 'package:flutter/material.dart';
import '../../design/deep_space_theme.dart';

/// Sync stages matching Rust SyncStage enum
enum SyncStage {
  headers('Headers', 'Fetching block headers'),
  notes('Notes', 'Scanning for transactions'),
  witness('Witness', 'Building witness tree'),
  verify('Verify', 'Verifying commitments'),
  idle('Idle', 'Sync complete');

  const SyncStage(this.label, this.description);
  final String label;
  final String description;

  /// Get stage from string (FFI interop)
  static SyncStage fromString(String s) {
    switch (s.toLowerCase()) {
      case 'headers':
        return SyncStage.headers;
      case 'notes':
        return SyncStage.notes;
      case 'witness':
        return SyncStage.witness;
      case 'verify':
        return SyncStage.verify;
      default:
        return SyncStage.idle;
    }
  }

  /// Icon for stage
  IconData get icon {
    switch (this) {
      case SyncStage.headers:
        return Icons.cloud_download_outlined;
      case SyncStage.notes:
        return Icons.search;
      case SyncStage.witness:
        return Icons.account_tree_outlined;
      case SyncStage.verify:
        return Icons.verified_outlined;
      case SyncStage.idle:
        return Icons.check_circle_outline;
    }
  }
}

/// Sync status data model
class SyncStatus {
  final int localHeight;
  final int targetHeight;
  final double percent;
  final int? etaSeconds;
  final SyncStage stage;
  final int? lastCheckpointHeight;
  final DateTime? lastCheckpointTime;
  final double blocksPerSecond;
  final int notesDecrypted;
  final int lastBatchMs;
  final int commitmentsApplied;
  final bool isSyncing;

  const SyncStatus({
    required this.localHeight,
    required this.targetHeight,
    required this.percent,
    this.etaSeconds,
    required this.stage,
    this.lastCheckpointHeight,
    this.lastCheckpointTime,
    this.blocksPerSecond = 0,
    this.notesDecrypted = 0,
    this.lastBatchMs = 0,
    this.commitmentsApplied = 0,
    this.isSyncing = false,
  });

  /// Create idle status
  factory SyncStatus.idle({int height = 0}) => SyncStatus(
        localHeight: height,
        targetHeight: height,
        percent: 100,
        stage: SyncStage.idle,
        isSyncing: false,
      );

  /// Blocks remaining
  int get blocksRemaining => targetHeight - localHeight;

  /// Format ETA as human-readable string
  String get etaFormatted {
    if (etaSeconds == null || etaSeconds! <= 0) return '--';
    final secs = etaSeconds!;
    if (secs < 60) return '${secs}s';
    if (secs < 3600) return '${secs ~/ 60}m ${secs % 60}s';
    final hours = secs ~/ 3600;
    final mins = (secs % 3600) ~/ 60;
    return '${hours}h ${mins}m';
  }

  /// Format last checkpoint time
  String get lastCheckpointFormatted {
    if (lastCheckpointTime == null) return 'Never';
    final diff = DateTime.now().difference(lastCheckpointTime!);
    if (diff.inSeconds < 60) return '${diff.inSeconds}s ago';
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    if (diff.inHours < 24) return '${diff.inHours}h ago';
    return '${diff.inDays}d ago';
  }
}

/// Sync indicator widget with particle animation
class PSyncIndicator extends StatefulWidget {
  const PSyncIndicator({
    required this.status,
    this.compact = false,
    this.showParticles = true,
    this.onTap,
    super.key,
  });

  final SyncStatus status;
  final bool compact;
  final bool showParticles;
  final VoidCallback? onTap;

  @override
  State<PSyncIndicator> createState() => _PSyncIndicatorState();
}

class _PSyncIndicatorState extends State<PSyncIndicator>
    with TickerProviderStateMixin {
  late AnimationController _pulseController;
  late AnimationController _particleController;
  late Animation<double> _pulseAnimation;
  final List<_Particle> _particles = [];
  final math.Random _random = math.Random();

  @override
  void initState() {
    super.initState();

    _pulseController = AnimationController(
      duration: const Duration(milliseconds: 1500),
      vsync: this,
    )..repeat(reverse: true);

    _pulseAnimation = Tween<double>(begin: 0.8, end: 1.0).animate(
      CurvedAnimation(parent: _pulseController, curve: Curves.easeInOut),
    );

    _particleController = AnimationController(
      duration: const Duration(milliseconds: 50),
      vsync: this,
    )..repeat();

    _particleController.addListener(_updateParticles);
    _initParticles();
  }

  @override
  void dispose() {
    _pulseController.dispose();
    _particleController.dispose();
    super.dispose();
  }

  void _initParticles() {
    _particles.clear();
    for (int i = 0; i < 20; i++) {
      _particles.add(_Particle.random(_random));
    }
  }

  void _updateParticles() {
    if (!widget.status.isSyncing || !widget.showParticles) return;

    // Speed based on blocks/second (normalized to 0-1)
    final speed = (widget.status.blocksPerSecond / 200).clamp(0.1, 1.0);

    for (final particle in _particles) {
      particle.update(speed);
      if (particle.isDead) {
        particle.reset(_random);
      }
    }
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    if (widget.compact) {
      return _buildCompact();
    }
    return _buildFull();
  }

  Widget _buildCompact() {
    final status = widget.status;
    final isActive = status.isSyncing;

    return GestureDetector(
      onTap: widget.onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpacing.sm,
          vertical: AppSpacing.xs,
        ),
        decoration: BoxDecoration(
          color: isActive
              ? AppColors.gradientAStart.withValues(alpha: 0.15)
              : AppColors.surfaceElevated,
          borderRadius: BorderRadius.circular(8),
          border: Border.all(
            color: isActive
                ? AppColors.gradientAStart.withValues(alpha: 0.3)
                : AppColors.borderSubtle,
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (isActive)
              AnimatedBuilder(
                animation: _pulseAnimation,
                builder: (context, child) {
                  return Transform.scale(
                    scale: _pulseAnimation.value,
                    child: Icon(
                      Icons.sync,
                      size: 14,
                      color: AppColors.gradientAStart,
                    ),
                  );
                },
              )
            else
              Icon(
                Icons.check_circle,
                size: 14,
                color: AppColors.success,
              ),
            const SizedBox(width: 6),
            Text(
              isActive ? '${status.percent.toStringAsFixed(0)}%' : 'Synced',
              style: AppTypography.caption.copyWith(
                color: isActive
                    ? AppColors.gradientAStart
                    : AppColors.success,
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildFull() {
    final status = widget.status;
    final isActive = status.isSyncing;

    return GestureDetector(
      onTap: widget.onTap,
      child: Container(
        decoration: BoxDecoration(
          color: AppColors.surfaceElevated,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: AppColors.borderSubtle),
        ),
        clipBehavior: Clip.antiAlias,
        child: Stack(
          children: [
            // Particle layer
            if (isActive && widget.showParticles)
              Positioned.fill(
                child: CustomPaint(
                  painter: _ParticlePainter(
                    particles: _particles,
                    color: AppColors.gradientAStart,
                  ),
                ),
              ),

            // Content
            Padding(
              padding: const EdgeInsets.all(AppSpacing.md),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  // Header row
                  _buildHeader(status, isActive),

                  const SizedBox(height: AppSpacing.md),

                  // Progress bar
                  _buildProgressBar(status),

                  const SizedBox(height: AppSpacing.md),

                  // Stats row
                  _buildStats(status),

                  if (isActive) ...[
                    const SizedBox(height: AppSpacing.sm),
                    // Performance counters
                    _buildPerfCounters(status),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildHeader(SyncStatus status, bool isActive) {
    return Row(
      children: [
        // Stage icon with pulse
        AnimatedBuilder(
          animation: _pulseAnimation,
          builder: (context, child) {
            return Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: isActive
                    ? LinearGradient(
                        colors: [
                          AppColors.gradientAStart.withValues(
                            alpha: _pulseAnimation.value * 0.3,
                          ),
                          AppColors.gradientAEnd.withValues(
                            alpha: _pulseAnimation.value * 0.2,
                          ),
                        ],
                      )
                    : null,
                color: isActive ? null : AppColors.success.withValues(alpha: 0.2),
              ),
              child: Icon(
                status.stage.icon,
                size: 20,
                color: isActive
                    ? AppColors.gradientAStart
                    : AppColors.success,
              ),
            );
          },
        ),

        const SizedBox(width: AppSpacing.sm),

        // Stage info
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                isActive ? status.stage.label : 'Synced',
                style: AppTypography.body.copyWith(
                  color: AppColors.textPrimary,
                  fontWeight: FontWeight.w600,
                ),
              ),
              Text(
                isActive ? status.stage.description : 'Up to date',
                style: AppTypography.caption.copyWith(
                  color: AppColors.textMuted,
                ),
              ),
            ],
          ),
        ),

        // Percentage
        Text(
          '${status.percent.toStringAsFixed(1)}%',
          style: AppTypography.h3.copyWith(
            color: isActive
                ? AppColors.gradientAStart
                : AppColors.success,
            fontWeight: FontWeight.w700,
          ),
        ),
      ],
    );
  }

  Widget _buildProgressBar(SyncStatus status) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Stage indicators
        Row(
          children: SyncStage.values
              .where((s) => s != SyncStage.idle)
              .map((stage) {
            final isComplete = stage.index < status.stage.index;
            final isCurrent = stage == status.stage;

            return Expanded(
              child: Container(
                height: 4,
                margin: const EdgeInsets.symmetric(horizontal: 2),
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(2),
                  color: isComplete
                      ? AppColors.success
                      : isCurrent
                          ? AppColors.gradientAStart
                          : AppColors.nebula,
                ),
              ),
            );
          }).toList(),
        ),

        const SizedBox(height: AppSpacing.sm),

        // Main progress bar
        Stack(
          children: [
            // Background
            Container(
              height: 8,
              decoration: BoxDecoration(
                color: AppColors.nebula,
                borderRadius: BorderRadius.circular(4),
              ),
            ),

            // Progress
            AnimatedContainer(
              duration: const Duration(milliseconds: 300),
              curve: Curves.easeOutCubic,
              height: 8,
              width: double.infinity,
              child: FractionallySizedBox(
                alignment: Alignment.centerLeft,
                widthFactor: (status.percent / 100).clamp(0, 1),
                child: Container(
                  decoration: BoxDecoration(
                    gradient: AppColors.gradientALinear,
                    borderRadius: BorderRadius.circular(4),
                    boxShadow: status.isSyncing
                        ? [
                            BoxShadow(
                              color:
                                  AppColors.gradientAStart.withValues(alpha: 0.5),
                              blurRadius: 8,
                              spreadRadius: 1,
                            ),
                          ]
                        : null,
                  ),
                ),
              ),
            ),
          ],
        ),
      ],
    );
  }

  Widget _buildStats(SyncStatus status) {
    return Row(
      children: [
        _StatItem(
          label: 'Height',
          value: '${_formatNumber(status.localHeight)} / ${_formatNumber(status.targetHeight)}',
        ),
        const SizedBox(width: AppSpacing.lg),
        _StatItem(
          label: 'ETA',
          value: status.etaFormatted,
        ),
        const SizedBox(width: AppSpacing.lg),
        _StatItem(
          label: 'Checkpoint',
          value: status.lastCheckpointFormatted,
        ),
      ],
    );
  }

  Widget _buildPerfCounters(SyncStatus status) {
    return Container(
      padding: const EdgeInsets.all(AppSpacing.sm),
      decoration: BoxDecoration(
        color: AppColors.voidBlack.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: [
          _PerfCounter(
            label: 'blk/s',
            value: status.blocksPerSecond.toStringAsFixed(1),
            icon: Icons.speed,
          ),
          _PerfCounter(
            label: 'notes',
            value: _formatNumber(status.notesDecrypted),
            icon: Icons.note_outlined,
          ),
          _PerfCounter(
            label: 'batch',
            value: '${status.lastBatchMs}ms',
            icon: Icons.timer_outlined,
          ),
          _PerfCounter(
            label: 'commits',
            value: _formatNumber(status.commitmentsApplied),
            icon: Icons.commit,
          ),
        ],
      ),
    );
  }

  String _formatNumber(int n) {
    if (n >= 1000000) return '${(n / 1000000).toStringAsFixed(1)}M';
    if (n >= 1000) return '${(n / 1000).toStringAsFixed(1)}K';
    return n.toString();
  }
}

// =============================================================================
// Supporting Widgets
// =============================================================================

class _StatItem extends StatelessWidget {
  const _StatItem({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: AppTypography.caption.copyWith(
            color: AppColors.textMuted,
            fontSize: 10,
          ),
        ),
        Text(
          value,
          style: AppTypography.code.copyWith(
            color: AppColors.textSecondary,
            fontSize: 12,
          ),
        ),
      ],
    );
  }
}

class _PerfCounter extends StatelessWidget {
  const _PerfCounter({
    required this.label,
    required this.value,
    required this.icon,
  });

  final String label;
  final String value;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(
          icon,
          size: 12,
          color: AppColors.textMuted,
        ),
        const SizedBox(width: 4),
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              value,
              style: AppTypography.code.copyWith(
                color: AppColors.gradientAStart,
                fontSize: 11,
                fontWeight: FontWeight.w600,
              ),
            ),
            Text(
              label,
              style: AppTypography.caption.copyWith(
                color: AppColors.textMuted,
                fontSize: 9,
              ),
            ),
          ],
        ),
      ],
    );
  }
}

// =============================================================================
// Particle Animation
// =============================================================================

class _Particle {
  double x;
  double y;
  double vx;
  double vy;
  double size;
  double opacity;
  double life;
  double maxLife;

  _Particle({
    required this.x,
    required this.y,
    required this.vx,
    required this.vy,
    required this.size,
    required this.opacity,
    required this.life,
    required this.maxLife,
  });

  factory _Particle.random(math.Random random) {
    return _Particle(
      x: random.nextDouble(),
      y: random.nextDouble(),
      vx: (random.nextDouble() - 0.5) * 0.02,
      vy: -random.nextDouble() * 0.02 - 0.005,
      size: random.nextDouble() * 3 + 1,
      opacity: random.nextDouble() * 0.5 + 0.2,
      life: 0,
      maxLife: random.nextDouble() * 100 + 50,
    );
  }

  bool get isDead => life >= maxLife;

  void update(double speedMultiplier) {
    x += vx * speedMultiplier;
    y += vy * speedMultiplier;
    life += speedMultiplier;

    // Fade out near end of life
    if (life > maxLife * 0.7) {
      opacity *= 0.95;
    }
  }

  void reset(math.Random random) {
    x = random.nextDouble();
    y = 1.0 + random.nextDouble() * 0.2;
    vx = (random.nextDouble() - 0.5) * 0.02;
    vy = -random.nextDouble() * 0.02 - 0.005;
    size = random.nextDouble() * 3 + 1;
    opacity = random.nextDouble() * 0.5 + 0.2;
    life = 0;
    maxLife = random.nextDouble() * 100 + 50;
  }
}

class _ParticlePainter extends CustomPainter {
  final List<_Particle> particles;
  final Color color;

  _ParticlePainter({required this.particles, required this.color});

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()..style = PaintingStyle.fill;

    for (final particle in particles) {
      paint.color = color.withValues(alpha: particle.opacity.clamp(0, 1));
      canvas.drawCircle(
        Offset(particle.x * size.width, particle.y * size.height),
        particle.size,
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _ParticlePainter oldDelegate) => true;
}

// =============================================================================
// Sync Indicator Card (for Home Screen)
// =============================================================================

/// Compact sync indicator for home screen header
class PSyncIndicatorCard extends StatelessWidget {
  const PSyncIndicatorCard({
    required this.status,
    this.onTap,
    super.key,
  });

  final SyncStatus status;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return PSyncIndicator(
      status: status,
      compact: false,
      showParticles: true,
      onTap: onTap,
    );
  }
}

/// Mini sync badge for app bar
class PSyncBadge extends StatelessWidget {
  const PSyncBadge({
    required this.status,
    this.onTap,
    super.key,
  });

  final SyncStatus status;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return PSyncIndicator(
      status: status,
      compact: true,
      showParticles: false,
      onTap: onTap,
    );
  }
}

