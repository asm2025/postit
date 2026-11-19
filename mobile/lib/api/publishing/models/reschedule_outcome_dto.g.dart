// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reschedule_outcome_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RescheduleOutcomeDto _$RescheduleOutcomeDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['failed', 'moved'],
    disallowNullValues: const ['failed', 'moved'],
  );
  return RescheduleOutcomeDto(
    failed: (json['failed'] as List<dynamic>).map((e) => e as String).toList(),
    moved: (json['moved'] as List<dynamic>).map((e) => e as String).toList(),
    additionalProperties:
        RescheduleOutcomeDto._extras(json, 'additionalProperties') == null
        ? const {}
        : RescheduleOutcomeDto._readExtras(
            RescheduleOutcomeDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$RescheduleOutcomeDtoToJson(
  RescheduleOutcomeDto instance,
) => <String, dynamic>{'failed': instance.failed, 'moved': instance.moved};
