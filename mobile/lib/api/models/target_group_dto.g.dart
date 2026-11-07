// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_group_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetGroupDto _$TargetGroupDtoFromJson(Map<String, dynamic> json) =>
    TargetGroupDto(
      accountIds: (json['account_ids'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      id: json['id'] as String,
      name: json['name'] as String,
      updatedAt: DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$TargetGroupDtoToJson(TargetGroupDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'id': instance.id,
      'name': instance.name,
      'updated_at': instance.updatedAt.toIso8601String(),
    };
