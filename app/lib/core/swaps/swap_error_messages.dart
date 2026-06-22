/// Maps raw swap/KDF exceptions to friendly, user-facing copy.
///
/// Swap failures bubble up as raw exception strings (KDF RPC payloads,
/// orchestrator timeouts, FFI errors). Those are useful for logs but
/// intimidating in the UI, so we translate the common cases into plain
/// language with an actionable hint where possible.
library;

import '../i18n/arb_text_localizer.dart';

String friendlySwapError(Object? error) {
  if (error == null) return 'Something went wrong with the swap.'.tr;
  final raw = error.toString();
  final text = raw.toLowerCase();

  if (text.contains('cancelled') || text.contains('canceled')) {
    return 'Swap cancelled.'.tr;
  }

  if (text.contains('timed out waiting for your ltc deposit') ||
      text.contains('timed out waiting for your varrr deposit') ||
      (text.contains('timeout') && text.contains('deposit'))) {
    return "We didn't detect your deposit in time. If you already sent it, it may still be confirming - check the funding balance and try again, or refund it from there."
        .tr;
  }

  if (text.contains('timed out waiting for arrr')) {
    return "Your ARRR hasn't reached the swap engine yet. It may still be confirming on-chain. You can try again shortly."
        .tr;
  }

  if (text.contains('timed out waiting for the atomic swap')) {
    return 'The swap is taking longer than expected to settle. It may still complete - check your open orders and balances before retrying.'
        .tr;
  }

  if (text.contains('notsufficientbalance') ||
      text.contains('insufficient') ||
      text.contains('not enough')) {
    return "There isn't enough balance to complete this swap, including network fees. Try a smaller amount."
        .tr;
  }

  if (text.contains('no asks') ||
      text.contains('no bids') ||
      text.contains('orderbook') && text.contains('empty') ||
      text.contains('no liquidity')) {
    return "There aren't enough orders on the book to fill this trade right now. Try a smaller amount or check back shortly."
        .tr;
  }

  if (text.contains('not running') ||
      text.contains('failed to start') ||
      text.contains('rpc password')) {
    return "The swap engine isn't ready yet. Give it a moment to connect and try again."
        .tr;
  }

  if (text.contains('network') ||
      text.contains('connection') ||
      text.contains('socket') ||
      text.contains('host')) {
    return 'Network connection problem. Check your internet and try again.'.tr;
  }

  if (text.contains('deposit window expired')) {
    return 'The deposit window expired. Start a fresh quote before sending funds.'
        .tr;
  }

  if (text.contains('invalid address') ||
      text.contains('address') && text.contains('invalid')) {
    return 'The destination address was rejected. Double-check it and try again.'
        .tr;
  }

  // Fall back to the raw message if it is short and human-ish, otherwise a
  // generic line so we never dump a JSON blob at the user.
  final firstLine = raw.split('\n').first.trim();
  if (firstLine.isNotEmpty &&
      firstLine.length <= 140 &&
      !firstLine.contains('{') &&
      !firstLine.contains('Exception:')) {
    return firstLine;
  }
  return 'The swap could not be completed. Please try again.'.tr;
}
