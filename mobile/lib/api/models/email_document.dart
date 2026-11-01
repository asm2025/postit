// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_block.dart';
import 'email_language.dart';

part 'email_document.g.dart';

/// An email: subject, optional preview text, language, and body blocks.
@JsonSerializable()
class EmailDocument {
  const EmailDocument({
    required this.blocks,
    required this.language,
    required this.subject,
    this.preheader,
  });
  
  factory EmailDocument.fromJson(Map<String, Object?> json) => _$EmailDocumentFromJson(json);
  
  /// Body blocks, top to bottom.
  final List<EmailBlock> blocks;

  /// The campaign's language. Required: a contact's locale is often empty, so it cannot.
  /// decide the direction or the footer.
  final EmailLanguage language;

  /// Preview text shown next to the subject in most inboxes.
  final String? preheader;

  /// Subject line.
  final String subject;

  Map<String, Object?> toJson() => _$EmailDocumentToJson(this);
}
