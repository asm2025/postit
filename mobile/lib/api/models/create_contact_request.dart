// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'create_contact_request.g.dart';

/// A contact to add. An address already in the book updates that contact instead: fields.
/// given here replace the old values, custom `fields` merge key by key, and the rest stays.
@JsonSerializable()
class CreateContactRequest {
  const CreateContactRequest({
    required this.consentSource,
    required this.email,
    this.consentAt,
    this.fields,
    this.firstName,
    this.lastName,
    this.locale,
    this.tags,
  });
  
  factory CreateContactRequest.fromJson(Map<String, Object?> json) => _$CreateContactRequestFromJson(json);
  
  /// When consent was given; defaults to now.
  @JsonKey(name: 'consent_at')
  final DateTime? consentAt;

  /// Where the contact's consent came from ("signup form", "customer", "event"). Required.
  @JsonKey(name: 'consent_source')
  final String consentSource;

  /// The address.
  final String email;

  /// Custom merge fields.
  final dynamic fields;

  /// Given name.
  @JsonKey(name: 'first_name')
  final String? firstName;

  /// Family name.
  @JsonKey(name: 'last_name')
  final String? lastName;

  /// The contact's language.
  final String? locale;

  /// Tags.
  final List<String>? tags;

  Map<String, Object?> toJson() => _$CreateContactRequestToJson(this);
}
