// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'upload_offset_dto.g.dart';

/// Where an upload stands after a chunk.
@JsonSerializable()
class UploadOffsetDto {
  const UploadOffsetDto({
    required this.offset,
  });
  
  factory UploadOffsetDto.fromJson(Map<String, Object?> json) => _$UploadOffsetDtoFromJson(json);
  
  /// Bytes stored so far; the next chunk starts here.
  final int offset;

  Map<String, Object?> toJson() => _$UploadOffsetDtoToJson(this);
}
