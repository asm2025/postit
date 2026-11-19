// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_contact_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateContactRequest _$CreateContactRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'consent_at',
      'consent_source',
      'email',
      'fields',
      'first_name',
      'last_name',
      'locale',
      'tags',
    ],
    requiredKeys: const ['consent_source', 'email'],
    disallowNullValues: const ['consent_source', 'email'],
  );
  return CreateContactRequest(
    consentAt: CreateContactRequest._read_consentAt(json['consent_at']),
    consentSource: json['consent_source'] as String,
    email: json['email'] as String,
    fields: json['fields'] == null
        ? null
        : CreateContactRequestFields.fromJson(
            json['fields'] as Map<String, dynamic>,
          ),
    firstName: json['first_name'] as String?,
    lastName: json['last_name'] as String?,
    locale: json['locale'] as String?,
    tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
  );
}

Map<String, dynamic> _$CreateContactRequestToJson(
  CreateContactRequest instance,
) => <String, dynamic>{
  'consent_at': ?CreateContactRequest._write_consentAt(instance.consentAt),
  'consent_source': instance.consentSource,
  'email': instance.email,
  'fields': ?instance.fields?.toJson(),
  'first_name': ?instance.firstName,
  'last_name': ?instance.lastName,
  'locale': ?instance.locale,
  'tags': ?instance.tags,
};
