// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'disconnect_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DisconnectResponse _$DisconnectResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['cancelled_deliveries', 'native_scheduled_remaining'],
    disallowNullValues: const [
      'cancelled_deliveries',
      'native_scheduled_remaining',
    ],
  );
  return DisconnectResponse(
    cancelledDeliveries: DisconnectResponse._read_cancelledDeliveries(
      json['cancelled_deliveries'],
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
      'cancelled_deliveries': ?DisconnectResponse._write_cancelledDeliveries(
        instance.cancelledDeliveries,
      ),
      'native_scheduled_remaining':
          ?DisconnectResponse._write_nativeScheduledRemaining(
            instance.nativeScheduledRemaining,
          ),
    };
