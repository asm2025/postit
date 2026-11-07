// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'cancel_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CancelReportDto _$CancelReportDtoFromJson(Map<String, dynamic> json) =>
    CancelReportDto(
      cancelled: (json['cancelled'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      notCancellable: (json['not_cancellable'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      requested: (json['requested'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$CancelReportDtoToJson(CancelReportDto instance) =>
    <String, dynamic>{
      'cancelled': instance.cancelled,
      'not_cancellable': instance.notCancellable,
      'requested': instance.requested,
    };
