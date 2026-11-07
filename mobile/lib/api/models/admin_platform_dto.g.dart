// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_platform_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminPlatformDto _$AdminPlatformDtoFromJson(Map<String, dynamic> json) =>
    AdminPlatformDto(
      compiledIn: json['compiled_in'] as bool,
      configured: json['configured'] as bool,
      enabled: json['enabled'] as bool,
      id: json['id'] as String,
      name: json['name'] as String,
      redirectUri: json['redirect_uri'] as String?,
    );

Map<String, dynamic> _$AdminPlatformDtoToJson(AdminPlatformDto instance) =>
    <String, dynamic>{
      'compiled_in': instance.compiledIn,
      'configured': instance.configured,
      'enabled': instance.enabled,
      'id': instance.id,
      'name': instance.name,
      'redirect_uri': instance.redirectUri,
    };
