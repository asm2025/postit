// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'kind_switch_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

KindSwitchDto _$KindSwitchDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['kind', 'push'],
    disallowNullValues: const ['kind', 'push'],
  );
  return KindSwitchDto(
    email: json['email'] as bool?,
    kind: KindSwitchDto._read_kind(json['kind']),
    push: json['push'] as bool,
    additionalProperties:
        KindSwitchDto._extras(json, 'additionalProperties') == null
        ? const {}
        : KindSwitchDto._readExtras(
            KindSwitchDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$KindSwitchDtoToJson(KindSwitchDto instance) =>
    <String, dynamic>{
      'email': ?instance.email,
      'kind': ?KindSwitchDto._write_kind(instance.kind),
      'push': instance.push,
    };
