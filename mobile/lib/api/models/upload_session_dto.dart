// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'upload_session_dto.g.dart';

/// An open upload.
@JsonSerializable()
class UploadSessionDto {
  const UploadSessionDto({
    required this.chunkMax,
    required this.id,
    required this.offset,
    required this.size,
    this.expiresAt,
  });
  
  factory UploadSessionDto.fromJson(Map<String, Object?> json) => _$UploadSessionDtoFromJson(json);
  
  /// Largest chunk one `PUT` may carry.
  @JsonKey(name: 'chunk_max')
  final int chunkMax;

  /// When the session expires if left open.
  @JsonKey(name: 'expires_at')
  final DateTime? expiresAt;

  /// The asset ID the upload becomes.
  final String id;

  /// Bytes stored so far.
  final int offset;

  /// The declared size.
  final int size;

  Map<String, Object?> toJson() => _$UploadSessionDtoToJson(this);
}
