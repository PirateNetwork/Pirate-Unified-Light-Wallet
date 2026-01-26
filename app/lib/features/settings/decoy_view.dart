import 'package:flutter/material.dart';
import '../../design/compat.dart';
import '../../design/tokens/spacing.dart';
import '../../design/tokens/typography.dart';

/// Decoy wallet view shown when panic PIN is entered
class DecoyView extends StatelessWidget {
  const DecoyView({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: SafeArea(
        child: Column(
          children: [
            _buildHeader(),
            Expanded(
              child: _buildContent(),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildHeader() {
    return Container(
      padding: EdgeInsets.all(PSpacing.lg),
      child: Row(
        children: [
          Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: PirateTheme.accentColor.withValues(alpha: 0.2),
              shape: BoxShape.circle,
            ),
            child: Icon(
              Icons.account_balance_wallet,
              color: PirateTheme.accentColor,
              size: 20,
            ),
          ),
          SizedBox(width: PirateSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'My Wallet',
                  style: PirateTypography.bodyLarge.copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                Text(
                  'Tap to view details',
                  style: PirateTypography.bodySmall.copyWith(
                    color: Colors.grey[500],
                  ),
                ),
              ],
            ),
          ),
          IconButton(
            icon: Icon(Icons.settings, color: Colors.grey[400]),
            onPressed: () {
              // Show basic settings without sensitive options
            },
          ),
        ],
      ),
    );
  }

  Widget _buildContent() {
    return SingleChildScrollView(
      padding: EdgeInsets.all(PirateSpacing.lg),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _buildBalanceCard(),
          SizedBox(height: PirateSpacing.lg),
          _buildActionButtons(),
          SizedBox(height: PirateSpacing.xl),
          _buildRecentActivity(),
        ],
      ),
    );
  }

  Widget _buildBalanceCard() {
    return Container(
      padding: EdgeInsets.all(PirateSpacing.xl),
      decoration: BoxDecoration(
        gradient: LinearGradient(
          colors: [
            PirateTheme.accentColor.withValues(alpha: 0.2),
            PirateTheme.accentSecondary.withValues(alpha: 0.2),
          ],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        borderRadius: BorderRadius.circular(24),
        border: Border.all(
          color: PirateTheme.accentColor.withValues(alpha: 0.3),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Total Balance',
            style: PirateTypography.bodySmall.copyWith(
              color: Colors.grey[400],
            ),
          ),
          SizedBox(height: PirateSpacing.sm),
          Text(
            '0.05234 ARRR',
            style: PirateTypography.h1.copyWith(
              color: Colors.white,
              fontWeight: FontWeight.bold,
            ),
          ),
          SizedBox(height: PirateSpacing.xs),
          Text(
            r'≈ $2.45 USD',
            style: PirateTypography.body.copyWith(
              color: Colors.grey[500],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildActionButtons() {
    return Row(
      children: [
        Expanded(
          child: _buildActionButton(
            icon: Icons.arrow_upward,
            label: 'Send',
            color: Colors.red,
          ),
        ),
        SizedBox(width: PSpacing.md),
        Expanded(
          child: _buildActionButton(
            icon: Icons.arrow_downward,
            label: 'Receive',
            color: Colors.green,
          ),
        ),
        SizedBox(width: PirateSpacing.md),
        Expanded(
          child: _buildActionButton(
            icon: Icons.swap_horiz,
            label: 'Swap',
            color: Colors.blue,
          ),
        ),
      ],
    );
  }

  Widget _buildActionButton({
    required IconData icon,
    required String label,
    required Color color,
  }) {
    return Container(
      padding: EdgeInsets.symmetric(vertical: PSpacing.md),
      decoration: BoxDecoration(
        color: Colors.grey[900],
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: Colors.grey[800]!),
      ),
      child: Column(
        children: [
          Icon(icon, color: color, size: 28),
          SizedBox(height: PSpacing.xs),
          Text(
            label,
            style: PTypography.bodySmall().copyWith(
              color: Colors.grey[400],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildRecentActivity() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Recent Activity',
          style: PTypography.heading4().copyWith(
            color: Colors.white,
            fontWeight: FontWeight.w600,
          ),
        ),
        SizedBox(height: PSpacing.md),
        _buildActivityItem(
          type: 'Received',
          amount: '+0.02134 ARRR',
          date: '2 days ago',
          isPositive: true,
        ),
        _buildActivityItem(
          type: 'Sent',
          amount: '-0.01500 ARRR',
          date: '5 days ago',
          isPositive: false,
        ),
        _buildActivityItem(
          type: 'Received',
          amount: '+0.04600 ARRR',
          date: '1 week ago',
          isPositive: true,
        ),
      ],
    );
  }

  Widget _buildActivityItem({
    required String type,
    required String amount,
    required String date,
    required bool isPositive,
  }) {
    return Container(
      margin: EdgeInsets.only(bottom: PSpacing.sm),
      padding: EdgeInsets.all(PSpacing.md),
      decoration: BoxDecoration(
        color: Colors.grey[900],
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: Colors.grey[800]!),
      ),
      child: Row(
        children: [
          Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: (isPositive ? Colors.green : Colors.red).withValues(alpha: 0.1),
              shape: BoxShape.circle,
            ),
            child: Icon(
              isPositive ? Icons.arrow_downward : Icons.arrow_upward,
              color: isPositive ? Colors.green : Colors.red,
              size: 20,
            ),
          ),
          SizedBox(width: PSpacing.md),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  type,
                  style: PTypography.bodyMedium().copyWith(
                    color: Colors.white,
                    fontWeight: FontWeight.w500,
                  ),
                ),
                Text(
                  date,
                  style: PTypography.bodySmall().copyWith(
                    color: Colors.grey[500],
                  ),
                ),
              ],
            ),
          ),
          Text(
            amount,
            style: PTypography.bodyMedium().copyWith(
              color: isPositive ? Colors.green : Colors.white,
              fontWeight: FontWeight.w600,
            ),
          ),
        ],
      ),
    );
  }
}
