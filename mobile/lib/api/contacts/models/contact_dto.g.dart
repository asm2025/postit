// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactDto _$ContactDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'created_at',
      'email',
      'fields',
      'id',
      'status',
      'status_changed_at',
      'tags',
      'updated_at',
    ],
    disallowNullValues: const [
      'created_at',
      'email',
      'fields',
      'id',
      'status',
      'status_changed_at',
      'tags',
      'updated_at',
    ],
  );
  return ContactDto(
    consentAt: ContactDto._read_consentAt(json['consent_at']),
    consentSource: json['consent_source'] as String?,
    createdAt: ContactDto._read_createdAt(json['created_at']),
    email: json['email'] as String,
    fields: ContactDtoFields.fromJson(json['fields'] as Map<String, dynamic>),
    firstName: json['first_name'] as String?,
    id: json['id'] as String,
    lastName: json['last_name'] as String?,
    locale: json['locale'] as String?,
    status: ContactDto._read_status(json['status']),
    statusChangedAt: ContactDto._read_statusChangedAt(
      json['status_changed_at'],
    ),
    tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
    updatedAt: ContactDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        ContactDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactDto._readExtras(
            ContactDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactDtoToJson(ContactDto instance) =>
    <String, dynamic>{
      'consent_at': ?ContactDto._write_consentAt(instance.consentAt),
      'consent_source': ?instance.consentSource,
      'created_at': ?ContactDto._write_createdAt(instance.createdAt),
      'email': instance.email,
      'fields': instance.fields.toJson(),
      'first_name': ?instance.firstName,
      'id': instance.id,
      'last_name': ?instance.lastName,
      'locale': ?instance.locale,
      'status': ?ContactDto._write_status(instance.status),
      'status_changed_at': ?ContactDto._write_statusChangedAt(
        instance.statusChangedAt,
      ),
      'tags': instance.tags,
      'updated_at': ?ContactDto._write_updatedAt(instance.updatedAt),
    };
