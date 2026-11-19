// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'retry_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RetryRequest _$RetryRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['acknowledge_outcome_unknown', 'confirm'],
    disallowNullValues: const ['acknowledge_outcome_unknown', 'confirm'],
  );
  return RetryRequest(
    acknowledgeOutcomeUnknown: json['acknowledge_outcome_unknown'] as bool?,
    confirm: json['confirm'] as bool?,
  );
}

Map<String, dynamic> _$RetryRequestToJson(RetryRequest instance) =>
    <String, dynamic>{
      'acknowledge_outcome_unknown': ?instance.acknowledgeOutcomeUnknown,
      'confirm': ?instance.confirm,
    };
