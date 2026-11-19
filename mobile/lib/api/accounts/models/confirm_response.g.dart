// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'confirm_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConfirmResponse _$ConfirmResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['connected', 'skipped_elsewhere', 'unknown'],
    disallowNullValues: const ['connected', 'skipped_elsewhere', 'unknown'],
  );
  return ConfirmResponse(
    connected: (json['connected'] as List<dynamic>)
        .map((e) => ConnectedAccountDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    skippedElsewhere: ConfirmResponse._read_skippedElsewhere(
      json['skipped_elsewhere'],
    ),
    unknown: (json['unknown'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    additionalProperties:
        ConfirmResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : ConfirmResponse._readExtras(
            ConfirmResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ConfirmResponseToJson(ConfirmResponse instance) =>
    <String, dynamic>{
      'connected': instance.connected.map((e) => e.toJson()).toList(),
      'skipped_elsewhere': ?ConfirmResponse._write_skippedElsewhere(
        instance.skippedElsewhere,
      ),
      'unknown': instance.unknown,
    };
