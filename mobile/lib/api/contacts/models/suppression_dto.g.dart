// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'suppression_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SuppressionDto _$SuppressionDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['at', 'id', 'reason'],
    disallowNullValues: const ['at', 'id', 'reason'],
  );
  return SuppressionDto(
    at: SuppressionDto._read_at(json['at']),
    contactId: json['contact_id'] as String?,
    email: json['email'] as String?,
    id: json['id'] as String,
    reason: SuppressionDto._read_reason(json['reason']),
    additionalProperties:
        SuppressionDto._extras(json, 'additionalProperties') == null
        ? const {}
        : SuppressionDto._readExtras(
            SuppressionDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$SuppressionDtoToJson(SuppressionDto instance) =>
    <String, dynamic>{
      'at': ?SuppressionDto._write_at(instance.at),
      'contact_id': ?instance.contactId,
      'email': ?instance.email,
      'id': instance.id,
      'reason': ?SuppressionDto._write_reason(instance.reason),
    };
