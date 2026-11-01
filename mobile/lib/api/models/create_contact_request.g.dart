// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_contact_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateContactRequest _$CreateContactRequestFromJson(
  Map<String, dynamic> json,
) => CreateContactRequest(
  consentSource: json['consent_source'] as String,
  email: json['email'] as String,
  consentAt: json['consent_at'] == null
      ? null
      : DateTime.parse(json['consent_at'] as String),
  fields: json['fields'],
  firstName: json['first_name'] as String?,
  lastName: json['last_name'] as String?,
  locale: json['locale'] as String?,
  tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
);

Map<String, dynamic> _$CreateContactRequestToJson(
  CreateContactRequest instance,
) => <String, dynamic>{
  'consent_at': instance.consentAt?.toIso8601String(),
  'consent_source': instance.consentSource,
  'email': instance.email,
  'fields': instance.fields,
  'first_name': instance.firstName,
  'last_name': instance.lastName,
  'locale': instance.locale,
  'tags': instance.tags,
};
