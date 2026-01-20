// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'models.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;

/// @nodoc
mixin _$TunnelMode {
  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType && other is TunnelMode);
  }

  @override
  int get hashCode => runtimeType.hashCode;

  @override
  String toString() {
    return 'TunnelMode()';
  }
}

/// @nodoc
class $TunnelModeCopyWith<$Res> {
  $TunnelModeCopyWith(TunnelMode _, $Res Function(TunnelMode) __);
}

/// Adds pattern-matching-related methods to [TunnelMode].
extension TunnelModePatterns on TunnelMode {
  /// A variant of `map` that fallback to returning `orElse`.
  ///
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case final Subclass value:
  ///     return ...;
  ///   case _:
  ///     return orElse();
  /// }
  /// ```

  @optionalTypeArgs
  TResult maybeMap<TResult extends Object?>({
    TResult Function(TunnelMode_Tor value)? tor,
    TResult Function(TunnelMode_I2p value)? i2P,
    TResult Function(TunnelMode_Socks5 value)? socks5,
    TResult Function(TunnelMode_Direct value)? direct,
    required TResult orElse(),
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor() when tor != null:
        return tor(_that);
      case TunnelMode_I2p() when i2P != null:
        return i2P(_that);
      case TunnelMode_Socks5() when socks5 != null:
        return socks5(_that);
      case TunnelMode_Direct() when direct != null:
        return direct(_that);
      case _:
        return orElse();
    }
  }

  /// A `switch`-like method, using callbacks.
  ///
  /// Callbacks receives the raw object, upcasted.
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case final Subclass value:
  ///     return ...;
  ///   case final Subclass2 value:
  ///     return ...;
  /// }
  /// ```

  @optionalTypeArgs
  TResult map<TResult extends Object?>({
    required TResult Function(TunnelMode_Tor value) tor,
    required TResult Function(TunnelMode_I2p value) i2P,
    required TResult Function(TunnelMode_Socks5 value) socks5,
    required TResult Function(TunnelMode_Direct value) direct,
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor():
        return tor(_that);
      case TunnelMode_I2p():
        return i2P(_that);
      case TunnelMode_Socks5():
        return socks5(_that);
      case TunnelMode_Direct():
        return direct(_that);
    }
  }

  /// A variant of `map` that fallback to returning `null`.
  ///
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case final Subclass value:
  ///     return ...;
  ///   case _:
  ///     return null;
  /// }
  /// ```

  @optionalTypeArgs
  TResult? mapOrNull<TResult extends Object?>({
    TResult? Function(TunnelMode_Tor value)? tor,
    TResult? Function(TunnelMode_I2p value)? i2P,
    TResult? Function(TunnelMode_Socks5 value)? socks5,
    TResult? Function(TunnelMode_Direct value)? direct,
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor() when tor != null:
        return tor(_that);
      case TunnelMode_I2p() when i2P != null:
        return i2P(_that);
      case TunnelMode_Socks5() when socks5 != null:
        return socks5(_that);
      case TunnelMode_Direct() when direct != null:
        return direct(_that);
      case _:
        return null;
    }
  }

  /// A variant of `when` that fallback to an `orElse` callback.
  ///
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case Subclass(:final field):
  ///     return ...;
  ///   case _:
  ///     return orElse();
  /// }
  /// ```

  @optionalTypeArgs
  TResult maybeWhen<TResult extends Object?>({
    TResult Function()? tor,
    TResult Function()? i2P,
    TResult Function(String url)? socks5,
    TResult Function()? direct,
    required TResult orElse(),
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor() when tor != null:
        return tor();
      case TunnelMode_I2p() when i2P != null:
        return i2P();
      case TunnelMode_Socks5() when socks5 != null:
        return socks5(_that.url);
      case TunnelMode_Direct() when direct != null:
        return direct();
      case _:
        return orElse();
    }
  }

  /// A `switch`-like method, using callbacks.
  ///
  /// As opposed to `map`, this offers destructuring.
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case Subclass(:final field):
  ///     return ...;
  ///   case Subclass2(:final field2):
  ///     return ...;
  /// }
  /// ```

  @optionalTypeArgs
  TResult when<TResult extends Object?>({
    required TResult Function() tor,
    required TResult Function() i2P,
    required TResult Function(String url) socks5,
    required TResult Function() direct,
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor():
        return tor();
      case TunnelMode_I2p():
        return i2P();
      case TunnelMode_Socks5():
        return socks5(_that.url);
      case TunnelMode_Direct():
        return direct();
    }
  }

  /// A variant of `when` that fallback to returning `null`
  ///
  /// It is equivalent to doing:
  /// ```dart
  /// switch (sealedClass) {
  ///   case Subclass(:final field):
  ///     return ...;
  ///   case _:
  ///     return null;
  /// }
  /// ```

  @optionalTypeArgs
  TResult? whenOrNull<TResult extends Object?>({
    TResult? Function()? tor,
    TResult? Function()? i2P,
    TResult? Function(String url)? socks5,
    TResult? Function()? direct,
  }) {
    final _that = this;
    switch (_that) {
      case TunnelMode_Tor() when tor != null:
        return tor();
      case TunnelMode_I2p() when i2P != null:
        return i2P();
      case TunnelMode_Socks5() when socks5 != null:
        return socks5(_that.url);
      case TunnelMode_Direct() when direct != null:
        return direct();
      case _:
        return null;
    }
  }
}

/// @nodoc

class TunnelMode_Tor extends TunnelMode {
  const TunnelMode_Tor() : super._();

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType && other is TunnelMode_Tor);
  }

  @override
  int get hashCode => runtimeType.hashCode;

  @override
  String toString() {
    return 'TunnelMode.tor()';
  }
}

/// @nodoc

class TunnelMode_I2p extends TunnelMode {
  const TunnelMode_I2p() : super._();

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType && other is TunnelMode_I2p);
  }

  @override
  int get hashCode => runtimeType.hashCode;

  @override
  String toString() {
    return 'TunnelMode.i2P()';
  }
}

/// @nodoc

class TunnelMode_Socks5 extends TunnelMode {
  const TunnelMode_Socks5({required this.url}) : super._();

  /// Proxy URL
  final String url;

  /// Create a copy of TunnelMode
  /// with the given fields replaced by the non-null parameter values.
  @JsonKey(includeFromJson: false, includeToJson: false)
  @pragma('vm:prefer-inline')
  $TunnelMode_Socks5CopyWith<TunnelMode_Socks5> get copyWith =>
      _$TunnelMode_Socks5CopyWithImpl<TunnelMode_Socks5>(this, _$identity);

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is TunnelMode_Socks5 &&
            (identical(other.url, url) || other.url == url));
  }

  @override
  int get hashCode => Object.hash(runtimeType, url);

  @override
  String toString() {
    return 'TunnelMode.socks5(url: $url)';
  }
}

/// @nodoc
abstract mixin class $TunnelMode_Socks5CopyWith<$Res>
    implements $TunnelModeCopyWith<$Res> {
  factory $TunnelMode_Socks5CopyWith(
          TunnelMode_Socks5 value, $Res Function(TunnelMode_Socks5) _then) =
      _$TunnelMode_Socks5CopyWithImpl;
  @useResult
  $Res call({String url});
}

/// @nodoc
class _$TunnelMode_Socks5CopyWithImpl<$Res>
    implements $TunnelMode_Socks5CopyWith<$Res> {
  _$TunnelMode_Socks5CopyWithImpl(this._self, this._then);

  final TunnelMode_Socks5 _self;
  final $Res Function(TunnelMode_Socks5) _then;

  /// Create a copy of TunnelMode
  /// with the given fields replaced by the non-null parameter values.
  @pragma('vm:prefer-inline')
  $Res call({
    Object? url = null,
  }) {
    return _then(TunnelMode_Socks5(
      url: null == url
          ? _self.url
          : url // ignore: cast_nullable_to_non_nullable
              as String,
    ));
  }
}

/// @nodoc

class TunnelMode_Direct extends TunnelMode {
  const TunnelMode_Direct() : super._();

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType && other is TunnelMode_Direct);
  }

  @override
  int get hashCode => runtimeType.hashCode;

  @override
  String toString() {
    return 'TunnelMode.direct()';
  }
}

// dart format on
