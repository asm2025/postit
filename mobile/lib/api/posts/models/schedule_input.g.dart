// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'schedule_input.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ScheduleInputObject _$ScheduleInputObjectFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['local'],
    disallowNullValues: const ['local'],
  );
  return ScheduleInputObject(
    local: json['local'] as String,
    zone: json['zone'] as String?,
    additionalProperties:
        ScheduleInputObject._extras(json, 'additionalProperties') == null
        ? const {}
        : ScheduleInputObject._readExtras(
            ScheduleInputObject._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ScheduleInputObjectToJson(
  ScheduleInputObject instance,
) => <String, dynamic>{'local': instance.local, 'zone': ?instance.zone};
