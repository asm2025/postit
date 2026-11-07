// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_report_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetReportDto _$TargetReportDtoFromJson(Map<String, dynamic> json) =>
    TargetReportDto(
      accountId: json['account_id'] as String,
      violations: (json['violations'] as List<dynamic>)
          .map((e) => Violation.fromJson(e as Map<String, dynamic>))
          .toList(),
      format: json['format'] == null
          ? null
          : FormatId.fromJson(json['format'] as String),
    );

Map<String, dynamic> _$TargetReportDtoToJson(TargetReportDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'format': _$FormatIdEnumMap[instance.format],
      'violations': instance.violations,
    };

const _$FormatIdEnumMap = {
  FormatId.text: 'text',
  FormatId.link: 'link',
  FormatId.image: 'image',
  FormatId.carousel: 'carousel',
  FormatId.video: 'video',
  FormatId.shortVideo: 'short_video',
  FormatId.story: 'story',
  FormatId.reel: 'reel',
  FormatId.htmlEmail: 'html_email',
  FormatId.$unknown: r'$unknown',
};
