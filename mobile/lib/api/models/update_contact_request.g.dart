// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_contact_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdateContactRequest _$UpdateContactRequestFromJson(
  Map<String, dynamic> json,
) => UpdateContactRequest(
  consentAt: json['consent_at'] == null
      ? null
      : DateTime.parse(json['consent_at'] as String),
  consentSource: json['consent_source'] as String?,
  fields: json['fields'],
  firstName: json['first_name'] as String?,
  lastName: json['last_name'] as String?,
  locale: json['locale'] as String?,
  status: json['status'] == null
      ? null
      : ContactStatusDto.fromJson(json['status'] as String),
  tags: (json['tags'] as List<dynamic>?)?.map((e) => e as String).toList(),
);

Map<String, dynamic> _$UpdateContactRequestToJson(
  UpdateContactRequest instance,
) => <String, dynamic>{
  'consent_at': instance.consentAt?.toIso8601String(),
  'consent_source': instance.consentSource,
  'fields': instance.fields,
  'first_name': instance.firstName,
  'last_name': instance.lastName,
  'locale': instance.locale,
  'status': _$ContactStatusDtoEnumMap[instance.status],
  'tags': instance.tags,
};

const _$ContactStatusDtoEnumMap = {
  ContactStatusDto.subscribed: 'subscribed',
  ContactStatusDto.unsubscribed: 'unsubscribed',
  ContactStatusDto.bounced: 'bounced',
  ContactStatusDto.complained: 'complained',
  ContactStatusDto.$unknown: r'$unknown',
};
