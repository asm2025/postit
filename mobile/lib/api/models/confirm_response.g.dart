// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'confirm_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConfirmResponse _$ConfirmResponseFromJson(Map<String, dynamic> json) =>
    ConfirmResponse(
      connected: (json['connected'] as List<dynamic>)
          .map((e) => ConnectedAccountDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      skippedElsewhere: (json['skipped_elsewhere'] as num).toInt(),
      unknown: (json['unknown'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$ConfirmResponseToJson(ConfirmResponse instance) =>
    <String, dynamic>{
      'connected': instance.connected,
      'skipped_elsewhere': instance.skippedElsewhere,
      'unknown': instance.unknown,
    };
