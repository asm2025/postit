// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'connect_guide.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConnectGuide _$ConnectGuideFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['steps'],
    disallowNullValues: const ['steps'],
  );
  return ConnectGuide(
    steps: (json['steps'] as List<dynamic>)
        .map((e) => Message.fromJson(e as Map<String, dynamic>))
        .toList(),
    url: json['url'] as String?,
    additionalProperties:
        ConnectGuide._extras(json, 'additionalProperties') == null
        ? const {}
        : ConnectGuide._readExtras(
            ConnectGuide._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ConnectGuideToJson(ConnectGuide instance) =>
    <String, dynamic>{
      'steps': instance.steps.map((e) => e.toJson()).toList(),
      'url': ?instance.url,
    };
