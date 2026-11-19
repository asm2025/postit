// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidationDto _$ValidationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['targets', 'valid', 'violations'],
    disallowNullValues: const ['targets', 'valid', 'violations'],
  );
  return ValidationDto(
    targets: (json['targets'] as List<dynamic>)
        .map((e) => TargetReportDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    valid: json['valid'] as bool,
    violations: (json['violations'] as List<dynamic>)
        .map((e) => Violation.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        ValidationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ValidationDto._readExtras(
            ValidationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ValidationDtoToJson(ValidationDto instance) =>
    <String, dynamic>{
      'targets': instance.targets.map((e) => e.toJson()).toList(),
      'valid': instance.valid,
      'violations': instance.violations.map((e) => e.toJson()).toList(),
    };
