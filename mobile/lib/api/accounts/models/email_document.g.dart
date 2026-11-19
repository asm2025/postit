// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_document.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailDocument _$EmailDocumentFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['blocks', 'language', 'subject'],
    disallowNullValues: const ['blocks', 'language', 'subject'],
  );
  return EmailDocument(
    blocks: (json['blocks'] as List<dynamic>)
        .map((e) => EmailBlock.fromJson(e as Map<String, dynamic>))
        .toList(),
    language: EmailDocument._read_language(json['language']),
    preheader: json['preheader'] as String?,
    subject: json['subject'] as String,
    additionalProperties:
        EmailDocument._extras(json, 'additionalProperties') == null
        ? const {}
        : EmailDocument._readExtras(
            EmailDocument._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$EmailDocumentToJson(EmailDocument instance) =>
    <String, dynamic>{
      'blocks': instance.blocks.map((e) => e.toJson()).toList(),
      'language': ?EmailDocument._write_language(instance.language),
      'preheader': ?instance.preheader,
      'subject': instance.subject,
    };
