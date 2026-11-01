// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'suppression_reason_dto.dart';

part 'suppression_dto.g.dart';

/// An address that is never emailed.
@JsonSerializable()
class SuppressionDto {
  const SuppressionDto({
    required this.at,
    required this.id,
    required this.reason,
    this.contactId,
    this.email,
  });
  
  factory SuppressionDto.fromJson(Map<String, Object?> json) => _$SuppressionDtoFromJson(json);
  
  /// When it was suppressed.
  final DateTime at;

  /// The contact with this address, while one exists.
  @JsonKey(name: 'contact_id')
  final String? contactId;

  /// That contact's address, while it exists.
  final String? email;

  /// Entry ID.
  final String id;

  /// Why it is suppressed.
  final SuppressionReasonDto reason;

  Map<String, Object?> toJson() => _$SuppressionDtoToJson(this);
}
