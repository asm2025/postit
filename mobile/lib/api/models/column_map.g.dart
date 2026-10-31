// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'column_map.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ColumnMap _$ColumnMapFromJson(Map<String, dynamic> json) => ColumnMap(
  consentSource: json['consent_source'] as String,
  email: json['email'] as String,
  consentAt: json['consent_at'] as String?,
  fields: (json['fields'] as Map<String, dynamic>?)?.map(
    (k, e) => MapEntry(k, e as String),
  ),
  firstName: json['first_name'] as String?,
  lastName: json['last_name'] as String?,
  locale: json['locale'] as String?,
  tags: json['tags'] as String?,
);

Map<String, dynamic> _$ColumnMapToJson(ColumnMap instance) => <String, dynamic>{
  'consent_at': instance.consentAt,
  'consent_source': instance.consentSource,
  'email': instance.email,
  'fields': instance.fields,
  'first_name': instance.firstName,
  'last_name': instance.lastName,
  'locale': instance.locale,
  'tags': instance.tags,
};
