// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reschedule_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RescheduleRequest _$RescheduleRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(json, allowedKeys: const ['at'], requiredKeys: const ['at']);
  return RescheduleRequest(at: RescheduleRequest._read_at(json['at']));
}

Map<String, dynamic> _$RescheduleRequestToJson(RescheduleRequest instance) =>
    <String, dynamic>{'at': RescheduleRequest._write_at(instance.at)};
