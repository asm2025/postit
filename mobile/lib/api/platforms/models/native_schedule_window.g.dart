// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'native_schedule_window.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

NativeScheduleWindow _$NativeScheduleWindowFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['max_lead_secs', 'min_lead_secs'],
    disallowNullValues: const ['max_lead_secs', 'min_lead_secs'],
  );
  return NativeScheduleWindow(
    maxLeadSecs: NativeScheduleWindow._read_maxLeadSecs(json['max_lead_secs']),
    minLeadSecs: NativeScheduleWindow._read_minLeadSecs(json['min_lead_secs']),
    additionalProperties:
        NativeScheduleWindow._extras(json, 'additionalProperties') == null
        ? const {}
        : NativeScheduleWindow._readExtras(
            NativeScheduleWindow._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$NativeScheduleWindowToJson(
  NativeScheduleWindow instance,
) => <String, dynamic>{
  'max_lead_secs': ?NativeScheduleWindow._write_maxLeadSecs(
    instance.maxLeadSecs,
  ),
  'min_lead_secs': ?NativeScheduleWindow._write_minLeadSecs(
    instance.minLeadSecs,
  ),
};
