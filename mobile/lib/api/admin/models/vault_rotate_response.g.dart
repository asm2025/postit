// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'vault_rotate_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

VaultRotateResponse _$VaultRotateResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['job_id'],
    disallowNullValues: const ['job_id'],
  );
  return VaultRotateResponse(
    jobId: json['job_id'] as String,
    additionalProperties:
        VaultRotateResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : VaultRotateResponse._readExtras(
            VaultRotateResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$VaultRotateResponseToJson(
  VaultRotateResponse instance,
) => <String, dynamic>{'job_id': instance.jobId};
