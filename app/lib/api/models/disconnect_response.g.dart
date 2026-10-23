// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'disconnect_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DisconnectResponse _$DisconnectResponseFromJson(Map<String, dynamic> json) =>
    DisconnectResponse(
      cancelledDeliveries: (json['cancelled_deliveries'] as num).toInt(),
      nativeScheduledRemaining: (json['native_scheduled_remaining'] as num)
          .toInt(),
    );

Map<String, dynamic> _$DisconnectResponseToJson(DisconnectResponse instance) =>
    <String, dynamic>{
      'cancelled_deliveries': instance.cancelledDeliveries,
      'native_scheduled_remaining': instance.nativeScheduledRemaining,
    };
