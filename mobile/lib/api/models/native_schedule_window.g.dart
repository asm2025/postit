// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'native_schedule_window.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

NativeScheduleWindow _$NativeScheduleWindowFromJson(
  Map<String, dynamic> json,
) => NativeScheduleWindow(
  maxLeadSecs: (json['max_lead_secs'] as num).toInt(),
  minLeadSecs: (json['min_lead_secs'] as num).toInt(),
);

Map<String, dynamic> _$NativeScheduleWindowToJson(
  NativeScheduleWindow instance,
) => <String, dynamic>{
  'max_lead_secs': instance.maxLeadSecs,
  'min_lead_secs': instance.minLeadSecs,
};
