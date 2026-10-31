// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportDto _$ContactImportDtoFromJson(Map<String, dynamic> json) =>
    ContactImportDto(
      attestedAt: DateTime.parse(json['attested_at'] as String),
      createdAt: DateTime.parse(json['created_at'] as String),
      id: json['id'] as String,
      report: ContactImportReportDto.fromJson(
        json['report'] as Map<String, dynamic>,
      ),
      rowsOk: (json['rows_ok'] as num).toInt(),
      rowsRejected: (json['rows_rejected'] as num).toInt(),
      rowsTotal: (json['rows_total'] as num).toInt(),
      status: ContactImportStatusDto.fromJson(json['status'] as String),
      updatedAt: DateTime.parse(json['updated_at'] as String),
      listId: json['list_id'] as String?,
    );

Map<String, dynamic> _$ContactImportDtoToJson(ContactImportDto instance) =>
    <String, dynamic>{
      'attested_at': instance.attestedAt.toIso8601String(),
      'created_at': instance.createdAt.toIso8601String(),
      'id': instance.id,
      'list_id': instance.listId,
      'report': instance.report,
      'rows_ok': instance.rowsOk,
      'rows_rejected': instance.rowsRejected,
      'rows_total': instance.rowsTotal,
      'status': _$ContactImportStatusDtoEnumMap[instance.status]!,
      'updated_at': instance.updatedAt.toIso8601String(),
    };

const _$ContactImportStatusDtoEnumMap = {
  ContactImportStatusDto.processing: 'processing',
  ContactImportStatusDto.completed: 'completed',
  ContactImportStatusDto.failed: 'failed',
  ContactImportStatusDto.$unknown: r'$unknown',
};
