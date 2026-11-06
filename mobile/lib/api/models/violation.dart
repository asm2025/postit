// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'suggested_fix.dart';
import 'violation_code.dart';

part 'violation.g.dart';

/// One rule a post broke, with the data a client needs to explain and fix it.
@JsonSerializable()
class Violation {
  const Violation({
    required this.code,
    this.fix,
    this.params,
    this.path,
  });
  
  factory Violation.fromJson(Map<String, Object?> json) => _$ViolationFromJson(json);
  
  /// Which rule.
  final ViolationCode code;

  /// A change that would resolve it, when one exists.
  final SuggestedFix? fix;

  /// Code-specific parameters (see [`ViolationCode`]).
  final dynamic params;

  /// Where in the post, e.g. `text`, `media[1]`, `options.privacy`.
  final String? path;

  Map<String, Object?> toJson() => _$ViolationToJson(this);
}
