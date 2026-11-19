// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_actions_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobActionsDto _$JobActionsDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['cancel', 'delete', 'retry'],
    disallowNullValues: const ['cancel', 'delete', 'retry'],
  );
  return JobActionsDto(
    cancel: json['cancel'] as bool,
    delete: json['delete'] as bool,
    retry: json['retry'] as bool,
    additionalProperties:
        JobActionsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : JobActionsDto._readExtras(
            JobActionsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobActionsDtoToJson(JobActionsDto instance) =>
    <String, dynamic>{
      'cancel': instance.cancel,
      'delete': instance.delete,
      'retry': instance.retry,
    };
