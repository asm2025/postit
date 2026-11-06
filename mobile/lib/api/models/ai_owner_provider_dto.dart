// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_model_dto.dart';
import 'ai_protocol_dto.dart';
import 'ai_provider_dto.dart';

part 'ai_owner_provider_dto.g.dart';

/// A provider as its owner sees it: the configuration without any secret.
@JsonSerializable()
class AiOwnerProviderDto {
  const AiOwnerProviderDto({
    required this.hasKey,
    required this.headerNames,
    required this.id,
    required this.isDefault,
    required this.models,
    required this.name,
    required this.preset,
    required this.protocol,
    required this.view,
    this.baseUrl,
    this.defaultModel,
  });
  
  factory AiOwnerProviderDto.fromJson(Map<String, Object?> json) => _$AiOwnerProviderDtoFromJson(json);
  
  /// Where requests go, when the provider sets its own address.
  @JsonKey(name: 'base_url')
  final String? baseUrl;

  /// The model used when a request names none.
  @JsonKey(name: 'default_model')
  final String? defaultModel;

  /// Whether a key or header values are stored.
  @JsonKey(name: 'has_key')
  final bool hasKey;

  /// Names of the headers sent with every request; their values are never shown.
  @JsonKey(name: 'header_names')
  final List<String> headerNames;

  /// Provider ID.
  final String id;

  /// Whether it is the owner's default provider.
  @JsonKey(name: 'is_default')
  final bool isDefault;

  /// The configured models.
  final List<AiModelDto> models;

  /// Display name.
  final String name;

  /// The preset key, or `custom` for a pasted entry.
  final String preset;

  /// Wire protocol.
  final AiProtocolDto protocol;
  final dynamic view;

  Map<String, Object?> toJson() => _$AiOwnerProviderDtoToJson(this);
}
