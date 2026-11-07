// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'put_override_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PutOverrideRequest _$PutOverrideRequestFromJson(Map<String, dynamic> json) =>
    PutOverrideRequest(
      options: json['options'],
      patch: json['patch'] == null
          ? null
          : PostOverride.fromJson(json['patch'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$PutOverrideRequestToJson(PutOverrideRequest instance) =>
    <String, dynamic>{'options': instance.options, 'patch': instance.patch};
