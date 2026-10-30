// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'mark_read_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MarkReadRequest _$MarkReadRequestFromJson(Map<String, dynamic> json) =>
    MarkReadRequest(
      all: json['all'] as bool?,
      ids: (json['ids'] as List<dynamic>?)?.map((e) => e as String).toList(),
    );

Map<String, dynamic> _$MarkReadRequestToJson(MarkReadRequest instance) =>
    <String, dynamic>{'all': instance.all, 'ids': instance.ids};
