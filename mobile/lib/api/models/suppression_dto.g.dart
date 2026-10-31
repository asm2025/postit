// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'suppression_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SuppressionDto _$SuppressionDtoFromJson(Map<String, dynamic> json) =>
    SuppressionDto(
      at: DateTime.parse(json['at'] as String),
      id: json['id'] as String,
      reason: SuppressionReasonDto.fromJson(json['reason'] as String),
      contactId: json['contact_id'] as String?,
      email: json['email'] as String?,
    );

Map<String, dynamic> _$SuppressionDtoToJson(SuppressionDto instance) =>
    <String, dynamic>{
      'at': instance.at.toIso8601String(),
      'contact_id': instance.contactId,
      'email': instance.email,
      'id': instance.id,
      'reason': _$SuppressionReasonDtoEnumMap[instance.reason]!,
    };

const _$SuppressionReasonDtoEnumMap = {
  SuppressionReasonDto.unsubscribed: 'unsubscribed',
  SuppressionReasonDto.bounced: 'bounced',
  SuppressionReasonDto.complained: 'complained',
  SuppressionReasonDto.manual: 'manual',
  SuppressionReasonDto.$unknown: r'$unknown',
};
