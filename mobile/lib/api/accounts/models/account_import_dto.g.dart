// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'account_import_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AccountImportDto _$AccountImportDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'commit',
      'counts',
      'id',
      'last_line',
      'last_row',
      'platform',
      'rows',
      'rows_done',
      'rows_total',
      'status',
    ],
    disallowNullValues: const [
      'commit',
      'counts',
      'id',
      'last_line',
      'last_row',
      'platform',
      'rows',
      'rows_done',
      'rows_total',
      'status',
    ],
  );
  return AccountImportDto(
    commit: json['commit'] as bool,
    counts: AccountImportDto._read_counts(json['counts']),
    id: json['id'] as String,
    lastLine: AccountImportDto._read_lastLine(json['last_line']),
    lastRow: AccountImportDto._read_lastRow(json['last_row']),
    platform: json['platform'] as String,
    rows: (json['rows'] as List<dynamic>)
        .map((e) => ImportRowDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    rowsDone: AccountImportDto._read_rowsDone(json['rows_done']),
    rowsTotal: AccountImportDto._read_rowsTotal(json['rows_total']),
    status: AccountImportDto._read_status(json['status']),
    stopped: AccountImportDto._read_stopped(json['stopped']),
    additionalProperties:
        AccountImportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AccountImportDto._readExtras(
            AccountImportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AccountImportDtoToJson(AccountImportDto instance) =>
    <String, dynamic>{
      'commit': instance.commit,
      'counts': ?AccountImportDto._write_counts(instance.counts),
      'id': instance.id,
      'last_line': ?AccountImportDto._write_lastLine(instance.lastLine),
      'last_row': ?AccountImportDto._write_lastRow(instance.lastRow),
      'platform': instance.platform,
      'rows': instance.rows.map((e) => e.toJson()).toList(),
      'rows_done': ?AccountImportDto._write_rowsDone(instance.rowsDone),
      'rows_total': ?AccountImportDto._write_rowsTotal(instance.rowsTotal),
      'status': ?AccountImportDto._write_status(instance.status),
      'stopped': ?AccountImportDto._write_stopped(instance.stopped),
    };
