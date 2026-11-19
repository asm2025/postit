// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'violation.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Violation _$ViolationFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['code'],
    disallowNullValues: const ['code', 'params'],
  );
  return Violation(
    code: Violation._read_code(json['code']),
    fix: json['fix'] == null
        ? null
        : SuggestedFix.fromJson(json['fix'] as Map<String, dynamic>),
    params: json['params'] == null
        ? null
        : ViolationParams.fromJson(json['params'] as Map<String, dynamic>),
    path: json['path'] as String?,
    additionalProperties:
        Violation._extras(json, 'additionalProperties') == null
        ? const {}
        : Violation._readExtras(
            Violation._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ViolationToJson(Violation instance) => <String, dynamic>{
  'code': ?Violation._write_code(instance.code),
  'fix': ?instance.fix?.toJson(),
  'params': ?instance.params?.toJson(),
  'path': ?instance.path,
};
