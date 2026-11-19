// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_rejection_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportRejectionDto _$ContactImportRejectionDtoFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'row'],
    disallowNullValues: const ['code', 'row'],
  );
  return ContactImportRejectionDto(
    code: json['code'] as String,
    row: ContactImportRejectionDto._read_row(json['row']),
    additionalProperties:
        ContactImportRejectionDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactImportRejectionDto._readExtras(
            ContactImportRejectionDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactImportRejectionDtoToJson(
  ContactImportRejectionDto instance,
) => <String, dynamic>{
  'code': instance.code,
  'row': ?ContactImportRejectionDto._write_row(instance.row),
};
