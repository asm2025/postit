// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publish_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishRequest _$PublishRequestFromJson(Map<String, dynamic> json) =>
    PublishRequest(
      targets: (json['targets'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      at: json['at'] == null
          ? null
          : ScheduleInput.fromJson(json['at'] as Map<String, dynamic>),
      confirm: json['confirm'] as bool?,
      dryRun: json['dry_run'] as bool?,
      nativeSchedule: json['native_schedule'] as bool?,
    );

Map<String, dynamic> _$PublishRequestToJson(PublishRequest instance) =>
    <String, dynamic>{
      'at': instance.at,
      'confirm': instance.confirm,
      'dry_run': instance.dryRun,
      'native_schedule': instance.nativeSchedule,
      'targets': instance.targets,
    };
