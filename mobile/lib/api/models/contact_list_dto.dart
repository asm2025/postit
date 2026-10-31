// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'contact_list_dto.g.dart';

/// A contact list.
@JsonSerializable()
class ContactListDto {
  const ContactListDto({
    required this.createdAt,
    required this.id,
    required this.members,
    required this.name,
    required this.updatedAt,
  });
  
  factory ContactListDto.fromJson(Map<String, Object?> json) => _$ContactListDtoFromJson(json);
  
  /// When it was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// List ID.
  final String id;

  /// How many contacts are in it.
  final int members;

  /// The list's name.
  final String name;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$ContactListDtoToJson(this);
}
