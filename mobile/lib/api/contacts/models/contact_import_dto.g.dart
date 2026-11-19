// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportDto _$ContactImportDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'attested_at',
      'created_at',
      'id',
      'report',
      'rows_ok',
      'rows_rejected',
      'rows_total',
      'status',
      'updated_at',
    ],
    disallowNullValues: const [
      'attested_at',
      'created_at',
      'id',
      'report',
      'rows_ok',
      'rows_rejected',
      'rows_total',
      'status',
      'updated_at',
    ],
  );
  return ContactImportDto(
    attestedAt: ContactImportDto._read_attestedAt(json['attested_at']),
    createdAt: ContactImportDto._read_createdAt(json['created_at']),
    id: json['id'] as String,
    listId: json['list_id'] as String?,
    report: ContactImportReportDto.fromJson(
      json['report'] as Map<String, dynamic>,
    ),
    rowsOk: ContactImportDto._read_rowsOk(json['rows_ok']),
    rowsRejected: ContactImportDto._read_rowsRejected(json['rows_rejected']),
    rowsTotal: ContactImportDto._read_rowsTotal(json['rows_total']),
    status: ContactImportDto._read_status(json['status']),
    updatedAt: ContactImportDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        ContactImportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactImportDto._readExtras(
            ContactImportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactImportDtoToJson(
  ContactImportDto instance,
) => <String, dynamic>{
  'attested_at': ?ContactImportDto._write_attestedAt(instance.attestedAt),
  'created_at': ?ContactImportDto._write_createdAt(instance.createdAt),
  'id': instance.id,
  'list_id': ?instance.listId,
  'report': instance.report.toJson(),
  'rows_ok': ?ContactImportDto._write_rowsOk(instance.rowsOk),
  'rows_rejected': ?ContactImportDto._write_rowsRejected(instance.rowsRejected),
  'rows_total': ?ContactImportDto._write_rowsTotal(instance.rowsTotal),
  'status': ?ContactImportDto._write_status(instance.status),
  'updated_at': ?ContactImportDto._write_updatedAt(instance.updatedAt),
};
