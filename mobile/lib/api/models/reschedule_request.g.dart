// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reschedule_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RescheduleRequest _$RescheduleRequestFromJson(Map<String, dynamic> json) =>
    RescheduleRequest(
      at: ScheduleInput.fromJson(json['at'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$RescheduleRequestToJson(RescheduleRequest instance) =>
    <String, dynamic>{'at': instance.at};
