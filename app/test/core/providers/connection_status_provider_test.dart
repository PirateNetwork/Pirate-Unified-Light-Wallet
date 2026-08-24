import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/features/settings/providers/endpoint_health_provider.dart';

void main() {
  test('validated endpoint does not wait for an initial sync target', () {
    expect(
      hasValidatedConnectionSignal(
        hasSyncStatus: false,
        endpointHealthPhase: EndpointHealthPhase.healthy,
      ),
      isTrue,
    );
  });

  test('unvalidated endpoint still reports connecting without sync status', () {
    expect(
      hasValidatedConnectionSignal(
        hasSyncStatus: false,
        endpointHealthPhase: EndpointHealthPhase.checking,
      ),
      isFalse,
    );
  });
}
