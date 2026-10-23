// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'discovered_account_dto.g.dart';

/// An account a pasted credential reaches.
@JsonSerializable()
class DiscoveredAccountDto {
  const DiscoveredAccountDto({
    required this.displayName,
    required this.remoteId,
    this.avatarUrl,
    this.handle,
  });
  
  factory DiscoveredAccountDto.fromJson(Map<String, Object?> json) => _$DiscoveredAccountDtoFromJson(json);
  
  /// Profile picture.
  @JsonKey(name: 'avatar_url')
  final String? avatarUrl;

  /// Name shown to people.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// The `@handle`, when the platform has one.
  final String? handle;

  /// The platform's ID for the account. Pass it to confirm to pick this account.
  @JsonKey(name: 'remote_id')
  final String remoteId;

  Map<String, Object?> toJson() => _$DiscoveredAccountDtoToJson(this);
}
