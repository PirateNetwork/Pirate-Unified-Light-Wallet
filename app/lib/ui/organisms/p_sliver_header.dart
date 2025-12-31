import 'package:flutter/material.dart';

/// Generic delegate for building custom sliver headers without relying on
/// Material's SliverAppBar implementation.
class PSliverHeaderDelegate extends SliverPersistentHeaderDelegate {
  PSliverHeaderDelegate({
    required this.maxExtentHeight,
    required this.minExtentHeight,
    required this.builder,
  });

  final double maxExtentHeight;
  final double minExtentHeight;
  final Widget Function(
    BuildContext context,
    double shrinkOffset,
    bool overlapsContent,
  ) builder;

  @override
  double get maxExtent => maxExtentHeight;

  @override
  double get minExtent => minExtentHeight;

  @override
  Widget build(
    BuildContext context,
    double shrinkOffset,
    bool overlapsContent,
  ) {
    return SizedBox.expand(
      child: builder(context, shrinkOffset, overlapsContent),
    );
  }

  @override
  bool shouldRebuild(covariant PSliverHeaderDelegate oldDelegate) {
    return maxExtentHeight != oldDelegate.maxExtentHeight ||
        minExtentHeight != oldDelegate.minExtentHeight ||
        oldDelegate.builder != builder;
  }
}

