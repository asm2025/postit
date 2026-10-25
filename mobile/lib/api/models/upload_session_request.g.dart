// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'upload_session_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UploadSessionRequest _$UploadSessionRequestFromJson(
  Map<String, dynamic> json,
) => UploadSessionRequest(
  filename: json['filename'] as String,
  size: (json['size'] as num).toInt(),
);

Map<String, dynamic> _$UploadSessionRequestToJson(
  UploadSessionRequest instance,
) => <String, dynamic>{'filename': instance.filename, 'size': instance.size};
