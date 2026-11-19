// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'kind_switch_update.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

KindSwitchUpdate _$KindSwitchUpdateFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['email', 'kind', 'push'],
    requiredKeys: const ['kind'],
    disallowNullValues: const ['kind'],
  );
  return KindSwitchUpdate(
    email: json['email'] as bool?,
    kind: KindSwitchUpdate._read_kind(json['kind']),
    push: json['push'] as bool?,
  );
}

Map<String, dynamic> _$KindSwitchUpdateToJson(KindSwitchUpdate instance) =>
    <String, dynamic>{
      'email': ?instance.email,
      'kind': ?KindSwitchUpdate._write_kind(instance.kind),
      'push': ?instance.push,
    };
