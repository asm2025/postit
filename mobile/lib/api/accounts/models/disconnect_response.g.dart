// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'disconnect_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DisconnectResponse _$DisconnectResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['canceled_deliveries', 'native_scheduled_remaining'],
    disallowNullValues: const [
      'canceled_deliveries',
      'native_scheduled_remaining',
    ],
  );
  return DisconnectResponse(
    canceledDeliveries: DisconnectResponse._read_canceledDeliveries(
      json['canceled_deliveries'],
    ),
    nativeScheduledRemaining: DisconnectResponse._read_nativeScheduledRemaining(
      json['native_scheduled_remaining'],
    ),
    additionalProperties:
        DisconnectResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : DisconnectResponse._readExtras(
            DisconnectResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DisconnectResponseToJson(DisconnectResponse instance) =>
    <String, dynamic>{
      'canceled_deliveries': ?DisconnectResponse._write_canceledDeliveries(
        instance.canceledDeliveries,
      ),
      'native_scheduled_remaining':
          ?DisconnectResponse._write_nativeScheduledRemaining(
            instance.nativeScheduledRemaining,
          ),
    };
