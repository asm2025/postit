// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelListDto _$AiModelListDtoFromJson(Map<String, dynamic> json) =>
    AiModelListDto(
      models: (json['models'] as List<dynamic>)
          .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      remoteError: json['remote_error'] == null
          ? null
          : AiFailureReasonDto.fromJson(json['remote_error'] as String),
    );

Map<String, dynamic> _$AiModelListDtoToJson(AiModelListDto instance) =>
    <String, dynamic>{
      'models': instance.models,
      'remote_error': _$AiFailureReasonDtoEnumMap[instance.remoteError],
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
