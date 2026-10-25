// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'media_kind_dto.dart';
import 'media_status_dto.dart';

part 'data4.g.dart';

@JsonSerializable()
class Data4 {
  const Data4({
    required this.bytes,
    required this.createdAt,
    required this.hasThumbnail,
    required this.id,
    required this.kind,
    required this.status,
    this.derivedFrom,
    this.durationMs,
    this.height,
    this.mime,
    this.originalFilename,
    this.sha256,
    this.width,
  });
  
  factory Data4.fromJson(Map<String, Object?> json) => _$Data4FromJson(json);
  
  /// The stored size in bytes.
  final int bytes;

  /// When it was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// The asset this one was fitted from.
  @JsonKey(name: 'derived_from')
  final String? derivedFrom;

  /// Duration in milliseconds, for video.
  @JsonKey(name: 'duration_ms')
  final int? durationMs;

  /// Whether `GET /media/{id}/thumbnail` has an image.
  @JsonKey(name: 'has_thumbnail')
  final bool hasThumbnail;

  /// Pixel height, once probed.
  final int? height;

  /// Asset ID.
  final String id;

  /// Image or video.
  final MediaKindDto kind;

  /// The MIME type the bytes sniffed as; unset until the upload completes.
  final String? mime;

  /// The name the uploaded file had. Metadata only; it never reaches a path.
  @JsonKey(name: 'original_filename')
  final String? originalFilename;

  /// Lowercase hex SHA-256 of the stored bytes, once known.
  final String? sha256;

  /// Where it is in its life.
  final MediaStatusDto status;

  /// Pixel width, once probed.
  final int? width;

  Map<String, Object?> toJson() => _$Data4ToJson(this);
}
