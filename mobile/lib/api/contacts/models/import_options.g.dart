// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_options.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportOptions _$ImportOptionsFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['attest_consent', 'columns', 'list_id'],
    requiredKeys: const ['attest_consent', 'columns'],
    disallowNullValues: const ['attest_consent', 'columns'],
  );
  return ImportOptions(
    attestConsent: json['attest_consent'] as bool,
    columns: ColumnMap.fromJson(json['columns'] as Map<String, dynamic>),
    listId: !json.containsKey('list_id')
        ? null
        : ImportOptions._read_listId(json['list_id']),
  );
}

Map<String, dynamic> _$ImportOptionsToJson(ImportOptions instance) =>
    <String, dynamic>{
      'attest_consent': instance.attestConsent,
      'columns': instance.columns.toJson(),
      if (instance.listId case final $value?)
        'list_id': ImportOptions._write_listId($value),
    };
