// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'aspect_ratio.dart';
import 'fit_fix.dart';
import 'fit_fix_type.dart';
import 'remove_media_fix.dart';
import 'remove_media_fix_type.dart';
import 'shorten_text_fix.dart';
import 'shorten_text_fix_type.dart';
import 'trim_fix.dart';
import 'trim_fix_type.dart';


part 'suggested_fix.g.dart';

/// A change that would make a violating post valid, offered to the client.
@JsonSerializable(createFactory: false)
sealed class SuggestedFix {
  const SuggestedFix();
  
  factory SuggestedFix.fromJson(Map<String, dynamic> json) =>
      SuggestedFixSealedDeserializer.tryDeserialize(json);
  
  Map<String, dynamic> toJson();
}

extension SuggestedFixSealedDeserializer on SuggestedFix {
  static SuggestedFix tryDeserialize(
    Map<String, dynamic> json, {
    String key = 'type',
    Map<Type, Object?>? mapping,
  }) {
    final mappingFallback = const <Type, Object?>{
      SuggestedFixFitFix: 'fit',
      SuggestedFixRemoveMediaFix: 'remove_media',
      SuggestedFixShortenTextFix: 'shorten_text',
      SuggestedFixTrimFix: 'trim',
    };
    final value = json[key];
    final effective = mapping ?? mappingFallback;
    return switch (value) {
      _ when value == effective[SuggestedFixFitFix] => SuggestedFixFitFix.fromJson(json),
      _ when value == effective[SuggestedFixRemoveMediaFix] => SuggestedFixRemoveMediaFix.fromJson(json),
      _ when value == effective[SuggestedFixShortenTextFix] => SuggestedFixShortenTextFix.fromJson(json),
      _ when value == effective[SuggestedFixTrimFix] => SuggestedFixTrimFix.fromJson(json),
      _ => throw FormatException('Unknown discriminator value "${json[key]}" for SuggestedFix'),
    };
  }
}

@JsonSerializable()
class SuggestedFixFitFix extends SuggestedFix implements FitFix {
  @override
  final AspectRatio aspect;
  @override
  final FitFixType type;

  const SuggestedFixFitFix({
    required this.aspect,
    required this.type,
  });
  
  factory SuggestedFixFitFix.fromJson(Map<String, dynamic> json) =>
      _$SuggestedFixFitFixFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$SuggestedFixFitFixToJson(this);
}
@JsonSerializable()
class SuggestedFixRemoveMediaFix extends SuggestedFix implements RemoveMediaFix {
  @override
  final int? index;
  @override
  final RemoveMediaFixType type;

  const SuggestedFixRemoveMediaFix({
    required this.index,
    required this.type,
  });
  
  factory SuggestedFixRemoveMediaFix.fromJson(Map<String, dynamic> json) =>
      _$SuggestedFixRemoveMediaFixFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$SuggestedFixRemoveMediaFixToJson(this);
}
@JsonSerializable()
class SuggestedFixShortenTextFix extends SuggestedFix implements ShortenTextFix {
  @override
  final int max;
  @override
  final ShortenTextFixType type;

  const SuggestedFixShortenTextFix({
    required this.max,
    required this.type,
  });
  
  factory SuggestedFixShortenTextFix.fromJson(Map<String, dynamic> json) =>
      _$SuggestedFixShortenTextFixFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$SuggestedFixShortenTextFixToJson(this);
}
@JsonSerializable()
class SuggestedFixTrimFix extends SuggestedFix implements TrimFix {
  @override
  final int maxMs;
  @override
  final TrimFixType type;

  const SuggestedFixTrimFix({
    required this.maxMs,
    required this.type,
  });
  
  factory SuggestedFixTrimFix.fromJson(Map<String, dynamic> json) =>
      _$SuggestedFixTrimFixFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$SuggestedFixTrimFixToJson(this);
}
