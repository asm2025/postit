// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'last_error_dto.g.dart';

/// The classified error behind a failed or parked delivery.
@JsonSerializable()
class LastErrorDto {
  const LastErrorDto({
    required this.classValue,
    required this.code,
  });
  
  factory LastErrorDto.fromJson(Map<String, Object?> json) => _$LastErrorDtoFromJson(json);
  
  /// How the engine treated it, such as `retryable`.
  /// The name has been replaced because it contains a keyword. Original name: `class`.
  @JsonKey(name: 'class')
  final String classValue;

  /// The error's code, such as `rate_limited`.
  final String code;

  Map<String, Object?> toJson() => _$LastErrorDtoToJson(this);
}
