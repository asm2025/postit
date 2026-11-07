// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'post_override_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PostOverrideDto _$PostOverrideDtoFromJson(Map<String, dynamic> json) =>
    PostOverrideDto(
      accountId: json['account_id'] as String,
      options: json['options'],
      patch: PostOverride.fromJson(json['patch'] as Map<String, dynamic>),
      updatedAt: DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$PostOverrideDtoToJson(PostOverrideDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'options': instance.options,
      'patch': instance.patch,
      'updated_at': instance.updatedAt.toIso8601String(),
    };
