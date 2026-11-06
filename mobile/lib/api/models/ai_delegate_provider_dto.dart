// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_model_dto.dart';
import 'ai_provider_dto.dart';

part 'ai_delegate_provider_dto.g.dart';

/// A provider as a delegate sees it: names and models only.
@JsonSerializable()
class AiDelegateProviderDto {
  const AiDelegateProviderDto({
    required this.id,
    required this.isDefault,
    required this.models,
    required this.name,
    required this.preset,
    required this.view,
    this.defaultModel,
  });
  
  factory AiDelegateProviderDto.fromJson(Map<String, Object?> json) => _$AiDelegateProviderDtoFromJson(json);
  
  /// The model used when a request names none.
  @JsonKey(name: 'default_model')
  final String? defaultModel;

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
  final dynamic view;

  Map<String, Object?> toJson() => _$AiDelegateProviderDtoToJson(this);
}
