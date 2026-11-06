// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_test_result_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiTestResultDto _$AiTestResultDtoFromJson(Map<String, dynamic> json) =>
    AiTestResultDto(
      ok: json['ok'] as bool,
      modelsFound: (json['models_found'] as num?)?.toInt(),
      reason: json['reason'] == null
          ? null
          : AiFailureReasonDto.fromJson(json['reason'] as String),
    );

Map<String, dynamic> _$AiTestResultDtoToJson(AiTestResultDto instance) =>
    <String, dynamic>{
      'models_found': instance.modelsFound,
      'ok': instance.ok,
      'reason': _$AiFailureReasonDtoEnumMap[instance.reason],
    };

const _$AiFailureReasonDtoEnumMap = {
  AiFailureReasonDto.keyRejected: 'key_rejected',
  AiFailureReasonDto.modelNotFound: 'model_not_found',
  AiFailureReasonDto.rateLimited: 'rate_limited',
  AiFailureReasonDto.badRequest: 'bad_request',
  AiFailureReasonDto.unavailable: 'unavailable',
  AiFailureReasonDto.timeout: 'timeout',
  AiFailureReasonDto.addressRefused: 'address_refused',
  AiFailureReasonDto.invalidResponse: 'invalid_response',
  AiFailureReasonDto.invalidOutput: 'invalid_output',
  AiFailureReasonDto.$unknown: r'$unknown',
};
