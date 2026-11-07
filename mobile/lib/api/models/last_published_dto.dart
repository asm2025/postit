// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'last_published_dto.g.dart';

/// The version and time of the newest live delivery to an account.
@JsonSerializable()
class LastPublishedDto {
  const LastPublishedDto({
    required this.accountId,
    required this.postVersion,
    required this.publishedAt,
  });
  
  factory LastPublishedDto.fromJson(Map<String, Object?> json) => _$LastPublishedDtoFromJson(json);
  
  /// The account.
  @JsonKey(name: 'account_id')
  final String accountId;

  /// The post version that went out.
  @JsonKey(name: 'post_version')
  final int postVersion;

  /// When it went out.
  @JsonKey(name: 'published_at')
  final DateTime publishedAt;

  Map<String, Object?> toJson() => _$LastPublishedDtoToJson(this);
}
