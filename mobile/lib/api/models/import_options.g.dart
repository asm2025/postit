// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_options.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportOptions _$ImportOptionsFromJson(Map<String, dynamic> json) =>
    ImportOptions(
      attestConsent: json['attest_consent'] as bool,
      columns: ColumnMap.fromJson(json['columns'] as Map<String, dynamic>),
      listId: json['list_id'] as String?,
    );

Map<String, dynamic> _$ImportOptionsToJson(ImportOptions instance) =>
    <String, dynamic>{
      'attest_consent': instance.attestConsent,
      'columns': instance.columns,
      'list_id': instance.listId,
    };
