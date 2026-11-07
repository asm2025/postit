// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validate_post_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidatePostRequest _$ValidatePostRequestFromJson(Map<String, dynamic> json) =>
    ValidatePostRequest(
      at: json['at'] == null
          ? null
          : ScheduleInput.fromJson(json['at'] as Map<String, dynamic>),
      nativeSchedule: json['native_schedule'] as bool?,
      targets: (json['targets'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$ValidatePostRequestToJson(
  ValidatePostRequest instance,
) => <String, dynamic>{
  'at': instance.at,
  'native_schedule': instance.nativeSchedule,
  'targets': instance.targets,
};
