// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_button_block_type.dart';

part 'email_button_block.g.dart';

/// A call-to-action button.
@JsonSerializable()
class EmailButtonBlock {
  const EmailButtonBlock({
    required this.label,
    required this.type,
    required this.url,
  });
  
  factory EmailButtonBlock.fromJson(Map<String, Object?> json) => _$EmailButtonBlockFromJson(json);
  
  /// Button text.
  final String label;
  final EmailButtonBlockType type;

  /// Where the button goes.
  final String url;

  Map<String, Object?> toJson() => _$EmailButtonBlockToJson(this);
}
