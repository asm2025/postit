// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_document.dart';
import 'media_asset_id.dart';

part 'post_override.g.dart';

/// Changes one account makes to the shared post. Absent fields keep the post's value.
@JsonSerializable()
class PostOverride {
  const PostOverride({
    this.email,
    this.link,
    this.media,
    this.options,
    this.tags,
    this.text,
  });
  
  factory PostOverride.fromJson(Map<String, Object?> json) => _$PostOverrideFromJson(json);
  
  /// The email document, for an email account.
  final EmailDocument? email;

  /// Replacement link: absent keeps the post's link, `null` removes it.
  final String? link;

  /// A subset of the post's media, in the order to publish it.
  final List<MediaAssetId>? media;

  /// Values for the agent's options schema.
  final dynamic options;

  /// Replacement tags.
  final List<String>? tags;

  /// Replacement text.
  final String? text;

  Map<String, Object?> toJson() => _$PostOverrideToJson(this);
}
