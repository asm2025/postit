// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_document.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailDocument _$EmailDocumentFromJson(Map<String, dynamic> json) =>
    EmailDocument(
      blocks: (json['blocks'] as List<dynamic>)
          .map((e) => EmailBlock.fromJson(e as Map<String, dynamic>))
          .toList(),
      language: EmailLanguage.fromJson(json['language'] as String),
      subject: json['subject'] as String,
      preheader: json['preheader'] as String?,
    );

Map<String, dynamic> _$EmailDocumentToJson(EmailDocument instance) =>
    <String, dynamic>{
      'blocks': instance.blocks,
      'language': _$EmailLanguageEnumMap[instance.language]!,
      'preheader': instance.preheader,
      'subject': instance.subject,
    };

const _$EmailLanguageEnumMap = {
  EmailLanguage.enUs: 'en-US',
  EmailLanguage.$unknown: r'$unknown',
};
