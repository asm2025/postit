// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'upload_session_request.g.dart';

/// Starts a resumable upload.
@JsonSerializable()
class UploadSessionRequest {
  const UploadSessionRequest({
    required this.filename,
    required this.size,
  });
  
  factory UploadSessionRequest.fromJson(Map<String, Object?> json) => _$UploadSessionRequestFromJson(json);
  
  /// The file name, 1 to 255 characters. Its extension (`jpg`, `png`, `gif`, `webp`, `mp4`,.
  /// `m4v`, `mov`) picks the kind and so the size limit.
  final String filename;

  /// The whole file's size in bytes.
  final int size;

  Map<String, Object?> toJson() => _$UploadSessionRequestToJson(this);
}
