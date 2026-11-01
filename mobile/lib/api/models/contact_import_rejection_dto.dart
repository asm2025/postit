// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'contact_import_rejection_dto.g.dart';

/// A row an import did not turn into a contact.
@JsonSerializable()
class ContactImportRejectionDto {
  const ContactImportRejectionDto({
    required this.code,
    required this.row,
  });
  
  factory ContactImportRejectionDto.fromJson(Map<String, Object?> json) => _$ContactImportRejectionDtoFromJson(json);
  
  /// Why: `email_invalid`, `consent_missing`, `consent_at_invalid`, `row_malformed`,.
  /// `duplicate_in_file`, or `contact_limit_exceeded`.
  final String code;

  /// The row's position in the file; the header is row 1.
  final int row;

  Map<String, Object?> toJson() => _$ContactImportRejectionDtoToJson(this);
}
