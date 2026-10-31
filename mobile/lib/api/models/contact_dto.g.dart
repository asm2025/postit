// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactDto _$ContactDtoFromJson(Map<String, dynamic> json) => ContactDto(
  createdAt: DateTime.parse(json['created_at'] as String),
  email: json['email'] as String,
  fields: json['fields'],
  id: json['id'] as String,
  status: ContactStatusDto.fromJson(json['status'] as String),
  statusChangedAt: DateTime.parse(json['status_changed_at'] as String),
  tags: (json['tags'] as List<dynamic>).map((e) => e as String).toList(),
  updatedAt: DateTime.parse(json['updated_at'] as String),
  consentAt: json['consent_at'] == null
      ? null
      : DateTime.parse(json['consent_at'] as String),
  consentSource: json['consent_source'] as String?,
  firstName: json['first_name'] as String?,
  lastName: json['last_name'] as String?,
  locale: json['locale'] as String?,
);

Map<String, dynamic> _$ContactDtoToJson(ContactDto instance) =>
    <String, dynamic>{
      'consent_at': instance.consentAt?.toIso8601String(),
      'consent_source': instance.consentSource,
      'created_at': instance.createdAt.toIso8601String(),
      'email': instance.email,
      'fields': instance.fields,
      'first_name': instance.firstName,
      'id': instance.id,
      'last_name': instance.lastName,
      'locale': instance.locale,
      'status': _$ContactStatusDtoEnumMap[instance.status]!,
      'status_changed_at': instance.statusChangedAt.toIso8601String(),
      'tags': instance.tags,
      'updated_at': instance.updatedAt.toIso8601String(),
    };

const _$ContactStatusDtoEnumMap = {
  ContactStatusDto.subscribed: 'subscribed',
  ContactStatusDto.unsubscribed: 'unsubscribed',
  ContactStatusDto.bounced: 'bounced',
  ContactStatusDto.complained: 'complained',
  ContactStatusDto.$unknown: r'$unknown',
};
