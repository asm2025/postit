// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'import_row_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ImportRowDto _$ImportRowDtoFromJson(Map<String, dynamic> json) => ImportRowDto(
  accountIds: (json['account_ids'] as List<dynamic>)
      .map((e) => e as String)
      .toList(),
  displayNameIgnored: json['display_name_ignored'] as bool,
  row: (json['row'] as num).toInt(),
  status: ImportStatusDto.fromJson(json['status'] as String),
  violations: (json['violations'] as List<dynamic>)
      .map((e) => e as String)
      .toList(),
  field: json['field'] as String?,
);

Map<String, dynamic> _$ImportRowDtoToJson(ImportRowDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'display_name_ignored': instance.displayNameIgnored,
      'field': instance.field,
      'row': instance.row,
      'status': _$ImportStatusDtoEnumMap[instance.status]!,
      'violations': instance.violations,
    };

const _$ImportStatusDtoEnumMap = {
  ImportStatusDto.ok: 'ok',
  ImportStatusDto.invalidFields: 'invalid_fields',
  ImportStatusDto.invalidCredentials: 'invalid_credentials',
  ImportStatusDto.alreadyConnected: 'already_connected',
  ImportStatusDto.connectedElsewhere: 'connected_elsewhere',
  ImportStatusDto.nothingFound: 'nothing_found',
  ImportStatusDto.platformError: 'platform_error',
  ImportStatusDto.$unknown: r'$unknown',
};
