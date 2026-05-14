/// Estimates Pirate Chain wallet birthday heights from approximate dates.
///
/// Pirate targets a 60 second block time, but historic chain time has drifted
/// enough that genesis + 1,440 blocks/day can estimate heights above the real
/// tip. Month checkpoints are first blocks at/after 00:00 UTC on the first day
/// of the month, generated from lightwalletd compact block timestamps. Early
/// pre-Overwinter months fall back to genesis for maximum restore safety.
class BirthdayHeightEstimator {
  const BirthdayHeightEstimator._();

  static const int blocksPerDay = 1440;
  static const int safetyMarginBlocks = blocksPerDay;
  static const int minYear = 2018;
  static final DateTime pirateGenesisUtc = DateTime.utc(2018, 8, 29);

  static int estimateForMonth({
    required int year,
    required int month,
    int? latestHeight,
  }) {
    final selected = DateTime.utc(year, month, 1);
    if (selected.isBefore(pirateGenesisUtc)) {
      return 1;
    }

    final checkpoint = _checkpointFor(year, month);
    final estimate = checkpoint?.height ?? _estimateFromLatestCheckpoint(selected);
    final safeEstimate = (estimate - safetyMarginBlocks)
        .clamp(1, estimate)
        .toInt();

    if (latestHeight != null) {
      final safeTip = latestHeight > safetyMarginBlocks
          ? latestHeight - safetyMarginBlocks
          : latestHeight;
      return safeEstimate.clamp(1, safeTip < 1 ? 1 : safeTip).toInt();
    }
    return safeEstimate;
  }

  static _BirthdayCheckpoint? _checkpointFor(int year, int month) {
    for (final checkpoint in _checkpoints) {
      if (checkpoint.year == year && checkpoint.month == month) {
        return checkpoint;
      }
    }
    return null;
  }

  static int _estimateFromLatestCheckpoint(DateTime selected) {
    final latest = _checkpoints.last;
    final latestDate = DateTime.utc(latest.year, latest.month);
    final days = selected.difference(latestDate).inDays;
    if (days <= 0) {
      return latest.height;
    }
    return latest.height + (days * blocksPerDay);
  }
}

class _BirthdayCheckpoint {
  const _BirthdayCheckpoint(this.year, this.month, this.height);

  final int year;
  final int month;
  final int height;
}

const List<_BirthdayCheckpoint> _checkpoints = [
  _BirthdayCheckpoint(2018, 8, 1),
  _BirthdayCheckpoint(2018, 9, 1),
  _BirthdayCheckpoint(2018, 10, 1),
  _BirthdayCheckpoint(2018, 11, 1),
  _BirthdayCheckpoint(2018, 12, 1),
  _BirthdayCheckpoint(2019, 1, 177101),
  _BirthdayCheckpoint(2019, 2, 221314),
  _BirthdayCheckpoint(2019, 3, 261145),
  _BirthdayCheckpoint(2019, 4, 305073),
  _BirthdayCheckpoint(2019, 5, 347347),
  _BirthdayCheckpoint(2019, 6, 391433),
  _BirthdayCheckpoint(2019, 7, 434063),
  _BirthdayCheckpoint(2019, 8, 478079),
  _BirthdayCheckpoint(2019, 9, 522131),
  _BirthdayCheckpoint(2019, 10, 564700),
  _BirthdayCheckpoint(2019, 11, 608716),
  _BirthdayCheckpoint(2019, 12, 651188),
  _BirthdayCheckpoint(2020, 1, 695062),
  _BirthdayCheckpoint(2020, 2, 739086),
  _BirthdayCheckpoint(2020, 3, 780343),
  _BirthdayCheckpoint(2020, 4, 824601),
  _BirthdayCheckpoint(2020, 5, 867348),
  _BirthdayCheckpoint(2020, 6, 911337),
  _BirthdayCheckpoint(2020, 7, 953934),
  _BirthdayCheckpoint(2020, 8, 997492),
  _BirthdayCheckpoint(2020, 9, 1040060),
  _BirthdayCheckpoint(2020, 10, 1080588),
  _BirthdayCheckpoint(2020, 11, 1122900),
  _BirthdayCheckpoint(2020, 12, 1164052),
  _BirthdayCheckpoint(2021, 1, 1205369),
  _BirthdayCheckpoint(2021, 2, 1247535),
  _BirthdayCheckpoint(2021, 3, 1284898),
  _BirthdayCheckpoint(2021, 4, 1327065),
  _BirthdayCheckpoint(2021, 5, 1369299),
  _BirthdayCheckpoint(2021, 6, 1413565),
  _BirthdayCheckpoint(2021, 7, 1456312),
  _BirthdayCheckpoint(2021, 8, 1500267),
  _BirthdayCheckpoint(2021, 9, 1544306),
  _BirthdayCheckpoint(2021, 10, 1586881),
  _BirthdayCheckpoint(2021, 11, 1630910),
  _BirthdayCheckpoint(2021, 12, 1673438),
  _BirthdayCheckpoint(2022, 1, 1716675),
  _BirthdayCheckpoint(2022, 2, 1759828),
  _BirthdayCheckpoint(2022, 3, 1798169),
  _BirthdayCheckpoint(2022, 4, 1840234),
  _BirthdayCheckpoint(2022, 5, 1881829),
  _BirthdayCheckpoint(2022, 6, 1924541),
  _BirthdayCheckpoint(2022, 7, 1966719),
  _BirthdayCheckpoint(2022, 8, 2010374),
  _BirthdayCheckpoint(2022, 9, 2053936),
  _BirthdayCheckpoint(2022, 10, 2095705),
  _BirthdayCheckpoint(2022, 11, 2138331),
  _BirthdayCheckpoint(2022, 12, 2178085),
  _BirthdayCheckpoint(2023, 1, 2217128),
  _BirthdayCheckpoint(2023, 2, 2256614),
  _BirthdayCheckpoint(2023, 3, 2293982),
  _BirthdayCheckpoint(2023, 4, 2335047),
  _BirthdayCheckpoint(2023, 5, 2377745),
  _BirthdayCheckpoint(2023, 6, 2421982),
  _BirthdayCheckpoint(2023, 7, 2460728),
  _BirthdayCheckpoint(2023, 8, 2505475),
  _BirthdayCheckpoint(2023, 9, 2548875),
  _BirthdayCheckpoint(2023, 10, 2591023),
  _BirthdayCheckpoint(2023, 11, 2635473),
  _BirthdayCheckpoint(2023, 12, 2678558),
  _BirthdayCheckpoint(2024, 1, 2722892),
  _BirthdayCheckpoint(2024, 2, 2767326),
  _BirthdayCheckpoint(2024, 3, 2809146),
  _BirthdayCheckpoint(2024, 4, 2854103),
  _BirthdayCheckpoint(2024, 5, 2896603),
  _BirthdayCheckpoint(2024, 6, 2939719),
  _BirthdayCheckpoint(2024, 7, 2982283),
  _BirthdayCheckpoint(2024, 8, 3026827),
  _BirthdayCheckpoint(2024, 9, 3071588),
  _BirthdayCheckpoint(2024, 10, 3112679),
  _BirthdayCheckpoint(2024, 11, 3157153),
  _BirthdayCheckpoint(2024, 12, 3200097),
  _BirthdayCheckpoint(2025, 1, 3244465),
  _BirthdayCheckpoint(2025, 2, 3288853),
  _BirthdayCheckpoint(2025, 3, 3329025),
  _BirthdayCheckpoint(2025, 4, 3373439),
  _BirthdayCheckpoint(2025, 5, 3416346),
  _BirthdayCheckpoint(2025, 6, 3460742),
  _BirthdayCheckpoint(2025, 7, 3503757),
  _BirthdayCheckpoint(2025, 8, 3548198),
  _BirthdayCheckpoint(2025, 9, 3592408),
  _BirthdayCheckpoint(2025, 10, 3635360),
  _BirthdayCheckpoint(2025, 11, 3679780),
  _BirthdayCheckpoint(2025, 12, 3722744),
  _BirthdayCheckpoint(2026, 1, 3767111),
  _BirthdayCheckpoint(2026, 2, 3811369),
  _BirthdayCheckpoint(2026, 3, 3851502),
  _BirthdayCheckpoint(2026, 4, 3895924),
  _BirthdayCheckpoint(2026, 5, 3938662),
];
