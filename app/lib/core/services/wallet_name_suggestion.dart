/// Returns the next number for the standard locally generated wallet name.
///
/// Only names that exactly match `My ARRR Wallet <number>` participate. Custom
/// names and malformed suffixes never affect the sequence.
int nextArrrWalletNumber(Iterable<String> existingNames) {
  final pattern = RegExp(
    r'^My ARRR Wallet ([1-9][0-9]*)$',
    caseSensitive: false,
  );
  var highest = 0;

  for (final name in existingNames) {
    final match = pattern.firstMatch(name.trim());
    if (match == null) continue;
    final number = int.tryParse(match.group(1)!);
    if (number != null && number > highest) {
      highest = number;
    }
  }

  return highest + 1;
}
