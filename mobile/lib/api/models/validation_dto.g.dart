// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'validation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ValidationDto _$ValidationDtoFromJson(Map<String, dynamic> json) =>
    ValidationDto(
      targets: (json['targets'] as List<dynamic>)
          .map((e) => TargetReportDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      valid: json['valid'] as bool,
      violations: (json['violations'] as List<dynamic>)
          .map((e) => Violation.fromJson(e as Map<String, dynamic>))
          .toList(),
    );

Map<String, dynamic> _$ValidationDtoToJson(ValidationDto instance) =>
    <String, dynamic>{
      'targets': instance.targets,
      'valid': instance.valid,
      'violations': instance.violations,
    };
