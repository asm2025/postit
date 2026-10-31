// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_heading_block_type.dart';
import 'heading_level.dart';

part 'email_heading_block.g.dart';

/// A heading.
@JsonSerializable()
class EmailHeadingBlock {
  const EmailHeadingBlock({
    required this.level,
    required this.text,
    required this.type,
  });
  
  factory EmailHeadingBlock.fromJson(Map<String, Object?> json) => _$EmailHeadingBlockFromJson(json);
  
  /// Heading size.
  final HeadingLevel level;

  /// Heading text.
  final String text;
  final EmailHeadingBlockType type;

  Map<String, Object?> toJson() => _$EmailHeadingBlockToJson(this);
}
