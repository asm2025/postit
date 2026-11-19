// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_test_result_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiTestResultDto _$AiTestResultDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['ok'],
    disallowNullValues: const ['ok'],
  );
  return AiTestResultDto(
    modelsFound: AiTestResultDto._read_modelsFound(json['models_found']),
    ok: json['ok'] as bool,
    reason: AiTestResultDto._read_reason(json['reason']),
    additionalProperties:
        AiTestResultDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiTestResultDto._readExtras(
            AiTestResultDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiTestResultDtoToJson(AiTestResultDto instance) =>
    <String, dynamic>{
      'models_found': ?AiTestResultDto._write_modelsFound(instance.modelsFound),
      'ok': instance.ok,
      'reason': ?AiTestResultDto._write_reason(instance.reason),
    };
