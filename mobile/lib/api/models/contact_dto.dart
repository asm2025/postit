// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'contact_status_dto.dart';

part 'contact_dto.g.dart';

/// An email contact.
@JsonSerializable()
class ContactDto {
  const ContactDto({
    required this.createdAt,
    required this.email,
    required this.fields,
    required this.id,
    required this.status,
    required this.statusChangedAt,
    required this.tags,
    required this.updatedAt,
    this.consentAt,
    this.consentSource,
    this.firstName,
    this.lastName,
    this.locale,
  });
  
  factory ContactDto.fromJson(Map<String, Object?> json) => _$ContactDtoFromJson(json);
  
  /// When consent was given.
  @JsonKey(name: 'consent_at')
  final DateTime? consentAt;

  /// Where the contact's consent came from.
  @JsonKey(name: 'consent_source')
  final String? consentSource;

  /// When the contact was added.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// The address, normalized (trimmed, lowercase).
  final String email;

  /// Custom merge fields.
  final dynamic fields;

  /// Given name.
  @JsonKey(name: 'first_name')
  final String? firstName;

  /// Contact ID.
  final String id;

  /// Family name.
  @JsonKey(name: 'last_name')
  final String? lastName;

  /// The contact's language.
  final String? locale;

  /// Whether the contact may be emailed.
  final ContactStatusDto status;

  /// When the status last changed.
  @JsonKey(name: 'status_changed_at')
  final DateTime statusChangedAt;

  /// Tags.
  final List<String> tags;

  /// When the contact last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$ContactDtoToJson(this);
}
