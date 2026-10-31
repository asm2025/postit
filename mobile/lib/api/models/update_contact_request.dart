// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'contact_status_dto.dart';

part 'update_contact_request.g.dart';

/// Changes to a contact. Absent fields stay as they are; the address never changes.
@JsonSerializable()
class UpdateContactRequest {
  const UpdateContactRequest({
    this.consentAt,
    this.consentSource,
    this.fields,
    this.firstName,
    this.lastName,
    this.locale,
    this.status,
    this.tags,
  });
  
  factory UpdateContactRequest.fromJson(Map<String, Object?> json) => _$UpdateContactRequestFromJson(json);
  
  /// When consent was given.
  @JsonKey(name: 'consent_at')
  final DateTime? consentAt;

  /// Where consent came from; cannot be blank.
  @JsonKey(name: 'consent_source')
  final String? consentSource;

  /// The custom merge fields, replacing the old ones.
  final dynamic fields;

  /// A new given name, or `null` to clear it.
  @JsonKey(name: 'first_name')
  final String? firstName;

  /// A new family name, or `null` to clear it.
  @JsonKey(name: 'last_name')
  final String? lastName;

  /// A new language, or `null` to clear it.
  final String? locale;

  /// `subscribed` or `unsubscribed`; `bounced` and `complained` answer.
  /// `status_not_settable`. Subscribing a contact the owner unsubscribed is owner-only.
  final ContactStatusDto? status;

  /// The tags, replacing the old ones.
  final List<String>? tags;

  Map<String, Object?> toJson() => _$UpdateContactRequestToJson(this);
}
