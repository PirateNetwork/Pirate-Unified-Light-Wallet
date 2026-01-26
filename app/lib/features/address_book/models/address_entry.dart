/// Address book entry model
library;

import 'package:flutter/material.dart';

/// Color tag for address book entries
enum ColorTag {
  none(0, 'None', Color(0xFF6B7280)),
  red(1, 'Red', Color(0xFFEF4444)),
  orange(2, 'Orange', Color(0xFFF97316)),
  yellow(3, 'Yellow', Color(0xFFEAB308)),
  green(4, 'Green', Color(0xFF22C55E)),
  blue(5, 'Blue', Color(0xFF3B82F6)),
  purple(6, 'Purple', Color(0xFF8B5CF6)),
  pink(7, 'Pink', Color(0xFFEC4899)),
  gray(8, 'Gray', Color(0xFF6B7280));

  final int value;
  final String displayName;
  final Color color;

  const ColorTag(this.value, this.displayName, this.color);

  static ColorTag fromValue(int value) {
    return ColorTag.values.firstWhere(
      (t) => t.value == value,
      orElse: () => ColorTag.none,
    );
  }
}

/// Maximum label length
const int kMaxLabelLength = 100;

/// Maximum notes length
const int kMaxNotesLength = 500;

/// Address book entry
@immutable
class AddressEntry {
  final int id;
  final String walletId;
  final String address;
  final String label;
  final String? notes;
  final ColorTag colorTag;
  final bool isFavorite;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? lastUsedAt;
  final int useCount;

  const AddressEntry({
    required this.id,
    required this.walletId,
    required this.address,
    required this.label,
    this.notes,
    this.colorTag = ColorTag.none,
    this.isFavorite = false,
    required this.createdAt,
    required this.updatedAt,
    this.lastUsedAt,
    this.useCount = 0,
  });

  /// Create new entry for insertion
  factory AddressEntry.create({
    required String walletId,
    required String address,
    required String label,
    String? notes,
    ColorTag colorTag = ColorTag.none,
  }) {
    final now = DateTime.now();
    return AddressEntry(
      id: 0, // Will be set by database
      walletId: walletId,
      address: address,
      label: label,
      notes: notes,
      colorTag: colorTag,
      isFavorite: false,
      createdAt: now,
      updatedAt: now,
      useCount: 0,
    );
  }

  /// From JSON
  factory AddressEntry.fromJson(Map<String, dynamic> json) {
    return AddressEntry(
      id: json['id'] as int,
      walletId: json['wallet_id'] as String,
      address: json['address'] as String,
      label: json['label'] as String,
      notes: json['notes'] as String?,
      colorTag: ColorTag.fromValue(json['color_tag'] as int? ?? 0),
      isFavorite: json['is_favorite'] as bool? ?? false,
      createdAt: DateTime.parse(json['created_at'] as String),
      updatedAt: DateTime.parse(json['updated_at'] as String),
      lastUsedAt: json['last_used_at'] != null
          ? DateTime.parse(json['last_used_at'] as String)
          : null,
      useCount: json['use_count'] as int? ?? 0,
    );
  }

  /// To JSON
  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'wallet_id': walletId,
      'address': address,
      'label': label,
      'notes': notes,
      'color_tag': colorTag.value,
      'is_favorite': isFavorite,
      'created_at': createdAt.toIso8601String(),
      'updated_at': updatedAt.toIso8601String(),
      'last_used_at': lastUsedAt?.toIso8601String(),
      'use_count': useCount,
    };
  }

  /// Copy with modifications
  AddressEntry copyWith({
    int? id,
    String? walletId,
    String? address,
    String? label,
    String? notes,
    ColorTag? colorTag,
    bool? isFavorite,
    DateTime? createdAt,
    DateTime? updatedAt,
    DateTime? lastUsedAt,
    int? useCount,
  }) {
    return AddressEntry(
      id: id ?? this.id,
      walletId: walletId ?? this.walletId,
      address: address ?? this.address,
      label: label ?? this.label,
      notes: notes ?? this.notes,
      colorTag: colorTag ?? this.colorTag,
      isFavorite: isFavorite ?? this.isFavorite,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      lastUsedAt: lastUsedAt ?? this.lastUsedAt,
      useCount: useCount ?? this.useCount,
    );
  }

  /// Get truncated address for display
  String get truncatedAddress {
    if (address.length > 24) {
      return '${address.substring(0, 12)}...${address.substring(address.length - 12)}';
    }
    return address;
  }

  /// Get avatar letter
  String get avatarLetter {
    return label.isEmpty ? '?' : label[0].toUpperCase();
  }

  /// Validate entry
  List<String> validate() {
    final errors = <String>[];

    if (label.isEmpty) {
      errors.add('Label cannot be empty');
    } else if (label.length > kMaxLabelLength) {
      errors.add('Label too long (max $kMaxLabelLength characters)');
    }

    if (!address.startsWith('zs1')) {
      errors.add('Address must be a Sapling address (zs1...)');
    }

    if (notes != null && notes!.length > kMaxNotesLength) {
      errors.add('Notes too long (max $kMaxNotesLength characters)');
    }

    return errors;
  }

  /// Check if valid
  bool get isValid => validate().isEmpty;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AddressEntry &&
          runtimeType == other.runtimeType &&
          id == other.id &&
          address == other.address;

  @override
  int get hashCode => id.hashCode ^ address.hashCode;

  @override
  String toString() => 'AddressEntry($label, $truncatedAddress)';
}

