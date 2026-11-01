// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactListDto _$ContactListDtoFromJson(Map<String, dynamic> json) =>
    ContactListDto(
      createdAt: DateTime.parse(json['created_at'] as String),
      id: json['id'] as String,
      members: (json['members'] as num).toInt(),
      name: json['name'] as String,
      updatedAt: DateTime.parse(json['updated_at'] as String),
    );

Map<String, dynamic> _$ContactListDtoToJson(ContactListDto instance) =>
    <String, dynamic>{
      'created_at': instance.createdAt.toIso8601String(),
      'id': instance.id,
      'members': instance.members,
      'name': instance.name,
      'updated_at': instance.updatedAt.toIso8601String(),
    };
