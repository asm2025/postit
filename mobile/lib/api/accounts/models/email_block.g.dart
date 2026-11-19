// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'email_block.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

EmailHeadingBlock _$EmailHeadingBlockFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['level', 'text'],
    disallowNullValues: const ['level', 'text'],
  );
  return EmailHeadingBlock(
    level: EmailHeadingBlock._read_level(json['level']),
    text: json['text'] as String,
    additionalProperties:
        EmailHeadingBlock._extras(json, 'additionalProperties') == null
        ? const {}
        : EmailHeadingBlock._readExtras(
            EmailHeadingBlock._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$EmailHeadingBlockToJson(EmailHeadingBlock instance) =>
    <String, dynamic>{
      'level': ?EmailHeadingBlock._write_level(instance.level),
      'text': instance.text,
    };

EmailTextBlock _$EmailTextBlockFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['text'],
    disallowNullValues: const ['text'],
  );
  return EmailTextBlock(
    text: json['text'] as String,
    additionalProperties:
        EmailTextBlock._extras(json, 'additionalProperties') == null
        ? const {}
        : EmailTextBlock._readExtras(
            EmailTextBlock._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$EmailTextBlockToJson(EmailTextBlock instance) =>
    <String, dynamic>{'text': instance.text};

EmailImageBlock _$EmailImageBlockFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['alt', 'asset'],
    disallowNullValues: const ['alt'],
  );
  return EmailImageBlock(
    alt: json['alt'] as String,
    asset: EmailImageBlock._read_asset(json['asset']),
    link: json['link'] as String?,
    additionalProperties:
        EmailImageBlock._extras(json, 'additionalProperties') == null
        ? const {}
        : EmailImageBlock._readExtras(
            EmailImageBlock._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$EmailImageBlockToJson(EmailImageBlock instance) =>
    <String, dynamic>{
      'alt': instance.alt,
      'asset': EmailImageBlock._write_asset(instance.asset),
      'link': ?instance.link,
    };

EmailButtonBlock _$EmailButtonBlockFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['label', 'url'],
    disallowNullValues: const ['label', 'url'],
  );
  return EmailButtonBlock(
    label: json['label'] as String,
    url: json['url'] as String,
    additionalProperties:
        EmailButtonBlock._extras(json, 'additionalProperties') == null
        ? const {}
        : EmailButtonBlock._readExtras(
            EmailButtonBlock._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$EmailButtonBlockToJson(EmailButtonBlock instance) =>
    <String, dynamic>{'label': instance.label, 'url': instance.url};

EmailDividerBlock _$EmailDividerBlockFromJson(Map<String, dynamic> json) =>
    EmailDividerBlock(
      additionalProperties:
          EmailDividerBlock._extras(json, 'additionalProperties') == null
          ? const {}
          : EmailDividerBlock._readExtras(
              EmailDividerBlock._extras(json, 'additionalProperties'),
            ),
    );

Map<String, dynamic> _$EmailDividerBlockToJson(EmailDividerBlock instance) =>
    <String, dynamic>{};
