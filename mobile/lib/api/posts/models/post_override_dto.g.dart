// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_override_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostOverrideDto _$PostOverrideDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_id', 'options', 'patch', 'updated_at'],
    disallowNullValues: const ['account_id', 'options', 'patch', 'updated_at'],
  );
  return PostOverrideDto(
    accountId: json['account_id'] as String,
    options: PostOverrideDtoOptions.fromJson(
      json['options'] as Map<String, dynamic>,
    ),
    patch: PostOverride.fromJson(json['patch'] as Map<String, dynamic>),
    updatedAt: PostOverrideDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        PostOverrideDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PostOverrideDto._readExtras(
            PostOverrideDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PostOverrideDtoToJson(PostOverrideDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'options': instance.options.toJson(),
      'patch': instance.patch.toJson(),
      'updated_at': ?PostOverrideDto._write_updatedAt(instance.updatedAt),
    };
