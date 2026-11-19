// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'upload_session_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UploadSessionRequest _$UploadSessionRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['filename', 'size'],
    requiredKeys: const ['filename', 'size'],
    disallowNullValues: const ['filename', 'size'],
  );
  return UploadSessionRequest(
    filename: json['filename'] as String,
    size: UploadSessionRequest._read_size(json['size']),
  );
}

Map<String, dynamic> _$UploadSessionRequestToJson(
  UploadSessionRequest instance,
) => <String, dynamic>{
  'filename': instance.filename,
  'size': ?UploadSessionRequest._write_size(instance.size),
};
