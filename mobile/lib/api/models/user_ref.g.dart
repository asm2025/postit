// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_ref.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserRef _$UserRefFromJson(Map<String, dynamic> json) => UserRef(
  deleted: json['deleted'] as bool,
  id: json['id'] as String,
  displayName: json['display_name'] as String?,
);

Map<String, dynamic> _$UserRefToJson(UserRef instance) => <String, dynamic>{
  'deleted': instance.deleted,
  'display_name': instance.displayName,
  'id': instance.id,
};
