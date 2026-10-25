// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_actions_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobActionsDto _$JobActionsDtoFromJson(Map<String, dynamic> json) =>
    JobActionsDto(
      cancel: json['cancel'] as bool,
      delete: json['delete'] as bool,
      retry: json['retry'] as bool,
    );

Map<String, dynamic> _$JobActionsDtoToJson(JobActionsDto instance) =>
    <String, dynamic>{
      'cancel': instance.cancel,
      'delete': instance.delete,
      'retry': instance.retry,
    };
