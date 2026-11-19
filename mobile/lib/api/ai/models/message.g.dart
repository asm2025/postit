// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'message.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Message _$MessageFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['en', 'key'],
    disallowNullValues: const ['en', 'key'],
  );
  return Message(
    en: json['en'] as String,
    key: json['key'] as String,
    additionalProperties: Message._extras(json, 'additionalProperties') == null
        ? const {}
        : Message._readExtras(Message._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$MessageToJson(Message instance) => <String, dynamic>{
  'en': instance.en,
  'key': instance.key,
};
