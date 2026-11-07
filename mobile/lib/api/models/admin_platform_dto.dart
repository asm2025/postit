// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'admin_platform_dto.g.dart';

/// A platform as an admin sees it.
@JsonSerializable()
class AdminPlatformDto {
  const AdminPlatformDto({
    required this.compiledIn,
    required this.configured,
    required this.enabled,
    required this.id,
    required this.name,
    this.redirectUri,
  });
  
  factory AdminPlatformDto.fromJson(Map<String, Object?> json) => _$AdminPlatformDtoFromJson(json);
  
  /// Whether this build includes the platform's agent.
  @JsonKey(name: 'compiled_in')
  final bool compiledIn;

  /// Whether the deployment gives the platform the configuration it needs.
  final bool configured;

  /// Whether the platform is switched on.
  final bool enabled;

  /// The platform ID.
  final String id;

  /// Name shown to people.
  final String name;

  /// For an OAuth platform, the callback URL to register in its developer portal.
  @JsonKey(name: 'redirect_uri')
  final String? redirectUri;

  Map<String, Object?> toJson() => _$AdminPlatformDtoToJson(this);
}
