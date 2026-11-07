// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_group_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetGroupRequest _$TargetGroupRequestFromJson(Map<String, dynamic> json) =>
    TargetGroupRequest(
      name: json['name'] as String,
      accountIds: (json['account_ids'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$TargetGroupRequestToJson(TargetGroupRequest instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'name': instance.name,
    };
