/// Golden tests for Address Book screens
///
/// Tests list, detail, and edit views for visual regression.
/// Run with: flutter test --update-goldens test/golden/address_book_golden_test.dart
library;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:golden_toolkit/golden_toolkit.dart';

import '../../lib/design/theme.dart';
import '../../lib/features/address_book/address_book_screen.dart';
import '../../lib/features/address_book/models/address_entry.dart';
import '../../lib/features/address_book/providers/address_book_provider.dart';
import '../../lib/core/providers/wallet_providers.dart';
import '../golden_test_utils.dart';

/// Test wallet ID
const testWalletId = 'test_wallet';

/// Test data
final testEntries = [
  AddressEntry(
    id: 1,
    walletId: testWalletId,
    address: 'zs1abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890',
    label: 'Alice',
    notes: 'Coffee fund',
    colorTag: ColorTag.green,
    isFavorite: true,
    createdAt: DateTime(2024, 1, 1),
    updatedAt: DateTime(2024, 1, 15),
    lastUsedAt: DateTime(2024, 1, 15),
    useCount: 5,
  ),
  AddressEntry(
    id: 2,
    walletId: testWalletId,
    address: 'zs1xyz9876543210xyz9876543210xyz9876543210xyz9876543210xyz9876543210',
    label: "Bob's Hardware",
    colorTag: ColorTag.blue,
    createdAt: DateTime(2024, 1, 5),
    updatedAt: DateTime(2024, 1, 10),
    useCount: 2,
  ),
  AddressEntry(
    id: 3,
    walletId: testWalletId,
    address: 'zs1charlie890123456789012345678901234567890123456789012345678901234',
    label: 'Charlie (Exchange)',
    notes: 'Withdraw only - no deposits',
    colorTag: ColorTag.orange,
    createdAt: DateTime(2024, 1, 8),
    updatedAt: DateTime(2024, 1, 12),
    useCount: 1,
  ),
  AddressEntry(
    id: 4,
    walletId: testWalletId,
    address: 'zs1david567890123456789012345678901234567890123456789012345678901234',
    label: 'David',
    colorTag: ColorTag.purple,
    isFavorite: true,
    createdAt: DateTime(2024, 1, 10),
    updatedAt: DateTime(2024, 1, 10),
    useCount: 0,
  ),
];

/// Mock state notifier for testing
class MockAddressBookNotifier extends AddressBookNotifier {
  MockAddressBookNotifier(List<AddressEntry> entries) : super(testWalletId) {
    state = AddressBookState(entries: entries);
  }
}

/// Create test provider overrides
List<ProviderOverride> createTestOverrides(List<AddressEntry> entries) {
  return [
    addressBookProvider(testWalletId).overrideWith(
      () => MockAddressBookNotifier(entries),
    ),
    activeWalletProvider.overrideWith(
      () => _MockActiveWalletNotifier(),
    ),
  ];
}

class _MockActiveWalletNotifier extends ActiveWalletNotifier {
  _MockActiveWalletNotifier() : super() {
    state = testWalletId;
  }
}

void main() {
  group('Address Book Golden Tests', () {
    testGoldens('Address Book List - Desktop', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides(testEntries),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(1280, 800),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_list_desktop');
    });

    testGoldens('Address Book List - Mobile', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides(testEntries),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844), // iPhone 14 Pro
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_list_mobile');
    });

    testGoldens('Address Book Empty State', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides([]),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_empty_mobile');
    });

    testGoldens('Address Card - All Color Tags', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await loadAppFonts();

      // Create entries with all color tags
      final colorEntries = ColorTag.values.map((color) {
        return AddressEntry(
          id: color.value,
          walletId: testWalletId,
          address: 'zs1test${color.value}234567890123456789012345678901234567890123456789',
          label: 'Contact ${color.displayName}',
          colorTag: color,
          isFavorite: color == ColorTag.green || color == ColorTag.blue,
          createdAt: DateTime.now(),
          updatedAt: DateTime.now(),
          useCount: color.value,
        );
      }).toList();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides(colorEntries),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 900),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_color_tags');
    });

    testGoldens('Address Card Widget Variants', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        MaterialApp(
          theme: PTheme.dark(),
          home: Scaffold(
            body: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                children: [
                  // With notes and favorite
                  AddressCard(
                    entry: testEntries[0],
                    onTap: () {},
                    onFavoriteToggle: () {},
                  ),
                  const SizedBox(height: 16),
                  // Without notes, with color
                  AddressCard(
                    entry: testEntries[1],
                    onTap: () {},
                    onFavoriteToggle: () {},
                  ),
                  const SizedBox(height: 16),
                  // With notes, not favorite
                  AddressCard(
                    entry: testEntries[2],
                    onTap: () {},
                    onFavoriteToggle: () {},
                  ),
                ],
              ),
            ),
          ),
        ),
        surfaceSize: const Size(400, 400),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_card_variants');
    });

    testGoldens('Add Address Sheet', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides(testEntries),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: Scaffold(
              body: AddEditAddressSheet(
                walletId: testWalletId,
                onSave: (_) {},
              ),
            ),
          ),
        ),
        surfaceSize: const Size(390, 700),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_add_sheet');
    });

    testGoldens('Edit Address Sheet', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: createTestOverrides(testEntries),
          child: MaterialApp(
            theme: PTheme.dark(),
            home: Scaffold(
              body: AddEditAddressSheet(
                walletId: testWalletId,
                entry: testEntries[0],
                onSave: (_) {},
              ),
            ),
          ),
        ),
        surfaceSize: const Size(390, 700),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_edit_sheet');
    });

    testGoldens('Address Details Sheet', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        MaterialApp(
          theme: PTheme.dark(),
          home: Scaffold(
            body: AddressDetailsSheet(
              entry: testEntries[0],
              onEdit: () {},
              onDelete: () {},
              onSend: () {},
            ),
          ),
        ),
        surfaceSize: const Size(390, 500),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_details_sheet');
    });

    testGoldens('Filter Sheet', (tester) async {
      await loadAppFonts();

      final colorCounts = {
        ColorTag.green: 2,
        ColorTag.blue: 3,
        ColorTag.orange: 1,
        ColorTag.purple: 1,
        ColorTag.none: 5,
      };

      await tester.pumpWidgetBuilder(
        MaterialApp(
          theme: PTheme.dark(),
          home: Scaffold(
            body: FilterSheet(
              currentColor: ColorTag.green,
              showFavoritesOnly: false,
              colorCounts: colorCounts,
              onColorChanged: (_) {},
              onFavoritesToggled: () {},
              onClear: () {},
            ),
          ),
        ),
        surfaceSize: const Size(390, 400),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_filter_sheet');
    });

    testGoldens('Address Book Loading State', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier([]);
              notifier.state = const AddressBookState(isLoading: true);
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pump();

      await screenMatchesGolden(tester, 'address_book_loading');
    });

    testGoldens('Address Book Favorites Only', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier(testEntries);
              notifier.state = notifier.state.copyWith(showFavoritesOnly: true);
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_favorites_only');
    });

    testGoldens('Address Book with Color Filter', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier(testEntries);
              notifier.state = notifier.state.copyWith(filterColor: ColorTag.green);
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_color_filter');
    });

    testGoldens('Address Book with Search', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier(testEntries);
              notifier.state = notifier.state.copyWith(searchQuery: 'alice');
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      // Enter search text
      await tester.enterText(find.byType(TextField), 'alice');
      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_search_results');
    });

    testGoldens('Address Book No Search Results', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier(testEntries);
              notifier.state = notifier.state.copyWith(searchQuery: 'nonexistent');
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_no_search_results');
    });

    testGoldens('Address Book Error State', (tester) async {
      await loadAppFonts();

      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            addressBookProvider(testWalletId).overrideWith((ref) {
              final notifier = MockAddressBookNotifier([]);
              notifier.state = const AddressBookState(
                error: 'Failed to load address book',
              );
              return notifier;
            }),
            activeWalletProvider.overrideWith(
              (ref) => _MockActiveWalletNotifier(),
            ),
          ],
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const AddressBookScreen(),
          ),
        ),
        surfaceSize: const Size(390, 844),
      );

      await tester.pumpAndSettle();

      await screenMatchesGolden(tester, 'address_book_error');
    });
  });
}

