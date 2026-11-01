// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_divider_block_type.dart';

part 'email_divider_block.g.dart';

/// A horizontal rule.
@JsonSerializable()
class EmailDividerBlock {
  const EmailDividerBlock({
    required this.type,
  });
  
  factory EmailDividerBlock.fromJson(Map<String, Object?> json) => _$EmailDividerBlockFromJson(json);
  
  final EmailDividerBlockType type;

  Map<String, Object?> toJson() => _$EmailDividerBlockToJson(this);
}
