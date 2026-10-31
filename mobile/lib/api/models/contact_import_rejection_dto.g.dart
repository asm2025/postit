// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_rejection_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportRejectionDto _$ContactImportRejectionDtoFromJson(
  Map<String, dynamic> json,
) => ContactImportRejectionDto(
  code: json['code'] as String,
  row: (json['row'] as num).toInt(),
);

Map<String, dynamic> _$ContactImportRejectionDtoToJson(
  ContactImportRejectionDto instance,
) => <String, dynamic>{'code': instance.code, 'row': instance.row};
