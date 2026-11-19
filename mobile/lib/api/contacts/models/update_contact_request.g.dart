// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_contact_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdateContactRequest _$UpdateContactRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'consent_at',
      'consent_source',
      'fields',
      'first_name',
      'last_name',
      'locale',
      'status',
      'tags',
    ],
  );
  return UpdateContactRequest(
    consentAt: UpdateContactRequest._read_consentAt(json['consent_at']),
    consentSource: json['consent_source'] as String?,
    fields: json['fields'] == null
        ? null
        : UpdateContactRequestFields.fromJson(
            json['fields'] as Map<String, dynamic>,
          ),
    firstName: json['first_name'] as String?,
    lastName: json['last_name'] as String?,
    locale: json['locale'] as String?,
    status: UpdateContactRequest._read_status(json['status']),
    tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
  );
}

Map<String, dynamic> _$UpdateContactRequestToJson(
  UpdateContactRequest instance,
) => <String, dynamic>{
  'consent_at': ?UpdateContactRequest._write_consentAt(instance.consentAt),
  'consent_source': ?instance.consentSource,
  'fields': ?instance.fields?.toJson(),
  'first_name': ?instance.firstName,
  'last_name': ?instance.lastName,
  'locale': ?instance.locale,
  'status': ?UpdateContactRequest._write_status(instance.status),
  'tags': ?instance.tags,
};
