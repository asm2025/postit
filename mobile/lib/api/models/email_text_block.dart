// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_text_block_type.dart';

part 'email_text_block.g.dart';

/// A paragraph of text.
@JsonSerializable()
class EmailTextBlock {
  const EmailTextBlock({
    required this.text,
    required this.type,
  });
  
  factory EmailTextBlock.fromJson(Map<String, Object?> json) => _$EmailTextBlockFromJson(json);
  
  /// Paragraph text.
  final String text;
  final EmailTextBlockType type;

  Map<String, Object?> toJson() => _$EmailTextBlockToJson(this);
}
