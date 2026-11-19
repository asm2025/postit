// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_import_form.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactImportForm _$ContactImportFormFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['file', 'options'],
    disallowNullValues: const ['file', 'options'],
  );
  return ContactImportForm(
    file: json['file'] as String,
    options: ImportOptions.fromJson(json['options'] as Map<String, dynamic>),
    additionalProperties:
        ContactImportForm._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactImportForm._readExtras(
            ContactImportForm._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactImportFormToJson(ContactImportForm instance) =>
    <String, dynamic>{
      'file': instance.file,
      'options': instance.options.toJson(),
    };
