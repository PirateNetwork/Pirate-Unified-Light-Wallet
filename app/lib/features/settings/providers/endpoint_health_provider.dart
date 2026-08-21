import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../config/endpoints.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';
import 'transport_providers.dart';

enum EndpointHealthPhase {
  idle,
  checking,
  healthy,
  degraded,
  switching,
  offline,
}

@immutable
class EndpointHealthRecord {
  const EndpointHealthRecord({
    required this.url,
    required this.healthy,
    required this.checkedAt,
    this.height,
    this.responseTimeMs,
    this.chainName,
    this.error,
  });

  final String url;
  final bool healthy;
  final DateTime checkedAt;
  final int? height;
  final int? responseTimeMs;
  final String? chainName;
  final String? error;
}

@immutable
class EndpointHealthState {
  const EndpointHealthState({
    required this.phase,
    required this.records,
    this.activeUrl,
    this.switchedFrom,
    this.switchedTo,
  });

  const EndpointHealthState.idle()
    : phase = EndpointHealthPhase.idle,
      records = const <String, EndpointHealthRecord>{},
      activeUrl = null,
      switchedFrom = null,
      switchedTo = null;

  final EndpointHealthPhase phase;
  final Map<String, EndpointHealthRecord> records;
  final String? activeUrl;
  final String? switchedFrom;
  final String? switchedTo;

  EndpointHealthRecord? recordFor(String url) => records[_keyForUrl(url)];

  EndpointHealthState copyWith({
    EndpointHealthPhase? phase,
    Map<String, EndpointHealthRecord>? records,
    String? activeUrl,
    String? switchedFrom,
    String? switchedTo,
    bool clearSwitch = false,
  }) {
    return EndpointHealthState(
      phase: phase ?? this.phase,
      records: records ?? this.records,
      activeUrl: activeUrl ?? this.activeUrl,
      switchedFrom: clearSwitch ? null : switchedFrom ?? this.switchedFrom,
      switchedTo: clearSwitch ? null : switchedTo ?? this.switchedTo,
    );
  }
}

typedef LightdEndpointProbe = Future<NodeTestResult> Function({
  required String url,
  String? tlsPin,
});

final lightdEndpointProbeProvider = Provider<LightdEndpointProbe>((ref) {
  return FfiBridge.testNode;
});

final endpointHealthProvider =
    NotifierProvider.autoDispose<EndpointHealthNotifier, EndpointHealthState>(
      EndpointHealthNotifier.new,
    );

class EndpointHealthNotifier extends Notifier<EndpointHealthState> {
  static const Duration _checkInterval = Duration(minutes: 10);
  static const Duration _confirmationDelay = Duration(seconds: 5);
  static const Duration _startupDelay = Duration(seconds: 2);
  static const int _maximumHealthyTipLag = 24;

  Timer? _scheduledCheck;
  Timer? _periodicCheck;
  bool _checking = false;
  int _consecutiveFailures = 0;
  int _consecutiveStaleChecks = 0;
  int _checkGeneration = 0;
  String? _observedSelection;

  @override
  EndpointHealthState build() {
    ref
      ..listen<WalletId?>(activeWalletProvider, (_, next) {
        if (next != null) {
          _checkGeneration += 1;
          _scheduleCheck(_startupDelay, probePool: false);
        }
      })
      ..listen<AsyncValue<LightdEndpointConfig>>(lightdEndpointConfigProvider, (
        _,
        next,
      ) {
        final config = next.asData?.value;
        final selection = config == null
            ? null
            : '${config.url}|${config.automaticFailover}';
        if (selection != null && selection != _observedSelection) {
          final endpointChanged = _observedSelection != null;
          if (endpointChanged) _checkGeneration += 1;
          _observedSelection = selection;
          _resetFailureTracking();
          state = state.copyWith(
            phase: EndpointHealthPhase.idle,
            activeUrl: config!.url,
            clearSwitch: true,
          );
          _scheduleCheck(_startupDelay, probePool: false);
        }
      })
      ..listen<String>(
        transportConfigProvider.select((config) => config.mode),
        (_, _) {
          _checkGeneration += 1;
          _resetFailureTracking();
          _scheduleCheck(_startupDelay, probePool: false);
        },
      )
      ..listen<bool>(torStatusProvider.select((status) => status.isReady), (
        _,
        ready,
      ) {
        if (ready) _scheduleCheck(Duration.zero, probePool: false);
      })
      ..onDispose(() {
        _scheduledCheck?.cancel();
        _periodicCheck?.cancel();
      });

    _periodicCheck = Timer.periodic(_checkInterval, (_) {
      unawaited(checkNow());
    });
    _scheduleCheck(_startupDelay, probePool: false);
    return const EndpointHealthState.idle();
  }

  Future<void> checkNow({bool probePool = false}) async {
    if (_checking || ref.read(activeWalletProvider) == null) return;

    final mode = ref.read(transportConfigProvider).mode.toLowerCase();
    if (mode == 'tor' && !ref.read(torStatusProvider).isReady) {
      _scheduleCheck(_confirmationDelay, probePool: probePool);
      return;
    }

    _checking = true;
    final generation = _checkGeneration;
    _scheduledCheck?.cancel();
    try {
      final config = await ref.read(lightdEndpointConfigProvider.future);
      if (!ref.mounted || generation != _checkGeneration) return;
      final current = LightdEndpoint.tryParse(
        config.url,
        tlsPin: config.tlsPin,
        automaticFailover: config.automaticFailover,
      );
      if (current == null) {
        state = state.copyWith(
          phase: EndpointHealthPhase.offline,
          activeUrl: config.url,
        );
        return;
      }

      _observedSelection = '${current.url}|${current.automaticFailover}';
      state = state.copyWith(
        phase: EndpointHealthPhase.checking,
        activeUrl: current.url,
        clearSwitch: true,
      );

      final currentRecord = await _probe(current, tlsPin: config.tlsPin);
      if (!ref.mounted || generation != _checkGeneration) return;
      _storeRecord(currentRecord);

      final preset = config.automaticFailover
          ? LightdEndpoint.currentAutomaticPreset(current)
          : LightdEndpoint.findPreset(current.url);
      final canFailOver =
          config.automaticFailover &&
          preset != null &&
          (config.tlsPin == null || config.tlsPin!.trim().isEmpty);
      final candidates = canFailOver
          ? LightdEndpoint.failoverCandidates(preset)
                .where((endpoint) => endpoint.url != current.url)
                .toList()
          : const <LightdEndpoint>[];

      final nextFailureCount = currentRecord.healthy
          ? 0
          : _consecutiveFailures + 1;
      final shouldProbePool =
          candidates.isNotEmpty && (probePool || nextFailureCount >= 2);
      final candidateRecords = shouldProbePool
          ? await Future.wait(candidates.map(_probe))
          : const <EndpointHealthRecord>[];
      if (!ref.mounted || generation != _checkGeneration) return;
      candidateRecords.forEach(_storeRecord);

      final healthyCandidates =
          <({LightdEndpoint endpoint, EndpointHealthRecord record})>[];
      for (var index = 0; index < candidateRecords.length; index++) {
        final record = candidateRecords[index];
        if (record.healthy) {
          healthyCandidates.add((endpoint: candidates[index], record: record));
        }
      }
      healthyCandidates.sort((a, b) {
        final heightComparison = (b.record.height ?? -1).compareTo(
          a.record.height ?? -1,
        );
        if (heightComparison != 0) return heightComparison;
        return candidates
            .indexOf(a.endpoint)
            .compareTo(candidates.indexOf(b.endpoint));
      });

      if (!currentRecord.healthy) {
        _consecutiveFailures = nextFailureCount;
        _consecutiveStaleChecks = 0;
        if (_consecutiveFailures < 2) {
          state = state.copyWith(phase: EndpointHealthPhase.degraded);
          _scheduleCheck(_confirmationDelay, probePool: true);
          return;
        }
        if (healthyCandidates.isNotEmpty) {
          _selectHealthyPoolMember(current, healthyCandidates.first.endpoint);
          return;
        }
        state = state.copyWith(phase: EndpointHealthPhase.offline);
        return;
      }

      _consecutiveFailures = 0;
      final best = healthyCandidates.isEmpty ? null : healthyCandidates.first;
      final currentHeight = currentRecord.height;
      final bestHeight = best?.record.height;
      final isStale =
          currentHeight != null &&
          bestHeight != null &&
          bestHeight - currentHeight > _maximumHealthyTipLag;
      if (isStale) {
        _consecutiveStaleChecks += 1;
        if (_consecutiveStaleChecks < 2) {
          state = state.copyWith(phase: EndpointHealthPhase.degraded);
          _scheduleCheck(_confirmationDelay, probePool: true);
          return;
        }
        _selectHealthyPoolMember(current, best!.endpoint);
        return;
      }

      _consecutiveStaleChecks = 0;
      state = state.copyWith(phase: EndpointHealthPhase.healthy);
    } catch (error) {
      if (!ref.mounted || generation != _checkGeneration) return;
      state = state.copyWith(phase: EndpointHealthPhase.degraded);
      _scheduleCheck(_confirmationDelay, probePool: false);
    } finally {
      _checking = false;
    }
  }

  Future<EndpointHealthRecord> _probe(
    LightdEndpoint endpoint, {
    String? tlsPin,
  }) async {
    final mode = ref.read(transportConfigProvider).mode.toLowerCase();
    final timeout = mode == 'direct'
        ? const Duration(seconds: 15)
        : const Duration(seconds: 45);
    try {
      final result = await ref
          .read(lightdEndpointProbeProvider)(
            url: endpoint.url,
            tlsPin: tlsPin ?? endpoint.tlsPin,
          )
          .timeout(timeout);
      final chain = result.chainName?.trim().toLowerCase();
      final expectedChain =
          endpoint.network != LightdNetwork.mainnet ||
          chain == null ||
          chain.isEmpty ||
          chain == 'main' ||
          chain == 'mainnet';
      return EndpointHealthRecord(
        url: endpoint.url,
        healthy: result.success && expectedChain,
        checkedAt: DateTime.now(),
        height: result.latestBlockHeight,
        responseTimeMs: result.responseTimeMs,
        chainName: result.chainName,
        error: expectedChain
            ? result.errorMessage
            : 'Endpoint reported an unexpected chain',
      );
    } catch (error) {
      return EndpointHealthRecord(
        url: endpoint.url,
        healthy: false,
        checkedAt: DateTime.now(),
        error: error.toString(),
      );
    }
  }

  void _selectHealthyPoolMember(
    LightdEndpoint current,
    LightdEndpoint replacement,
  ) {
    _resetFailureTracking();
    state = state.copyWith(
      phase: EndpointHealthPhase.healthy,
      activeUrl: replacement.url,
      switchedFrom: current.url,
      switchedTo: replacement.url,
    );
  }

  void _storeRecord(EndpointHealthRecord record) {
    state = state.copyWith(
      records: Map<String, EndpointHealthRecord>.unmodifiable({
        ...state.records,
        _keyForUrl(record.url): record,
      }),
    );
  }

  void _scheduleCheck(Duration delay, {required bool probePool}) {
    _scheduledCheck?.cancel();
    _scheduledCheck = Timer(delay, () {
      unawaited(checkNow(probePool: probePool));
    });
  }

  void _resetFailureTracking() {
    _consecutiveFailures = 0;
    _consecutiveStaleChecks = 0;
  }
}

String _keyForUrl(String url) {
  final parsed = LightdEndpoint.tryParse(url);
  return parsed?.url.toLowerCase() ?? url.trim().toLowerCase();
}
