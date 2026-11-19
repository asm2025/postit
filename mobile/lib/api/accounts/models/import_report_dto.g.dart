// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportReportDto _$ImportReportDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['commit', 'counts', 'rows', 'truncated'],
    disallowNullValues: const ['commit', 'counts', 'rows', 'truncated'],
  );
  return ImportReportDto(
    commit: json['commit'] as bool,
    counts: ImportReportDto._read_counts(json['counts']),
    rows: (json['rows'] as List<dynamic>)
        .map((e) => ImportRowDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    truncated: json['truncated'] as bool,
    additionalProperties:
        ImportReportDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ImportReportDto._readExtras(
            ImportReportDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ImportReportDtoToJson(ImportReportDto instance) =>
    <String, dynamic>{
      'commit': instance.commit,
      'counts': ?ImportReportDto._write_counts(instance.counts),
      'rows': instance.rows.map((e) => e.toJson()).toList(),
      'truncated': instance.truncated,
    };
