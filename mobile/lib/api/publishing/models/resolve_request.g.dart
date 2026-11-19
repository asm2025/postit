// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'resolve_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ResolveRequest _$ResolveRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['outcome'],
    requiredKeys: const ['outcome'],
    disallowNullValues: const ['outcome'],
  );
  return ResolveRequest(outcome: ResolveRequest._read_outcome(json['outcome']));
}

Map<String, dynamic> _$ResolveRequestToJson(ResolveRequest instance) =>
    <String, dynamic>{
      'outcome': ?ResolveRequest._write_outcome(instance.outcome),
    };
