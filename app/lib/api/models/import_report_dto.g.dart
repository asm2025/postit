// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportReportDto _$ImportReportDtoFromJson(Map<String, dynamic> json) =>
    ImportReportDto(
      commit: json['commit'] as bool,
      counts: Map<String, int>.from(json['counts'] as Map),
      rows: (json['rows'] as List<dynamic>)
          .map((e) => ImportRowDto.fromJson(e as Map<String, dynamic>))
          .toList(),
    );

Map<String, dynamic> _$ImportReportDtoToJson(ImportReportDto instance) =>
    <String, dynamic>{
      'commit': instance.commit,
      'counts': instance.counts,
      'rows': instance.rows,
    };
