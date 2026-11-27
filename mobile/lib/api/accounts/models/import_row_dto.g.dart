// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_row_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportRowDto _$ImportRowDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'account_ids',
      'display_name_ignored',
      'line',
      'row',
      'status',
      'violations',
    ],
    disallowNullValues: const [
      'account_ids',
      'display_name_ignored',
      'line',
      'row',
      'status',
      'violations',
    ],
  );
  return ImportRowDto(
    accountIds: (json['account_ids'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    displayNameIgnored: json['display_name_ignored'] as bool,
    field: json['field'] as String?,
    line: ImportRowDto._read_line(json['line']),
    row: ImportRowDto._read_row(json['row']),
    status: ImportRowDto._read_status(json['status']),
    violations: (json['violations'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    additionalProperties:
        ImportRowDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ImportRowDto._readExtras(
            ImportRowDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ImportRowDtoToJson(ImportRowDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'display_name_ignored': instance.displayNameIgnored,
      'field': ?instance.field,
      'line': ?ImportRowDto._write_line(instance.line),
      'row': ?ImportRowDto._write_row(instance.row),
      'status': ?ImportRowDto._write_status(instance.status),
      'violations': instance.violations,
    };
