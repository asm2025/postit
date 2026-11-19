// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'mark_read_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MarkReadResponse _$MarkReadResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['updated'],
    disallowNullValues: const ['updated'],
  );
  return MarkReadResponse(
    updated: MarkReadResponse._read_updated(json['updated']),
    additionalProperties:
        MarkReadResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : MarkReadResponse._readExtras(
            MarkReadResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$MarkReadResponseToJson(MarkReadResponse instance) =>
    <String, dynamic>{
      'updated': ?MarkReadResponse._write_updated(instance.updated),
    };
