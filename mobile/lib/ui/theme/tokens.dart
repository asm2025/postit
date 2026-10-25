import 'package:flutter/material.dart';

/// The P7.1 design tokens (`web/src/index.css`), for colors Material has no slot for.
@immutable
class PostitTokens extends ThemeExtension<PostitTokens> {
  const PostitTokens({
    required this.bg,
    required this.surface,
    required this.raised,
    required this.line,
    required this.text,
    required this.mutedText,
    required this.faint,
    required this.inkLogo,
    required this.pink,
    required this.pinkText,
    required this.pinkSoft,
    required this.onPink,
    required this.warnBg,
    required this.warnLine,
    required this.warnText,
    required this.warnIcon,
    required this.track,
    required this.disabled,
    required this.danger,
  });

  final Color bg, surface, raised, line, text, mutedText, faint, inkLogo;
  final Color pink, pinkText, pinkSoft, onPink;
  final Color warnBg, warnLine, warnText, warnIcon, track, disabled, danger;

  static const light = PostitTokens(
    bg: Color(0xFFF6F3F1),
    surface: Color(0xFFFFFFFF),
    raised: Color(0xFFF1ECE9),
    line: Color(0xFFE5DDD9),
    text: Color(0xFF1D1A1C),
    mutedText: Color(0xFF5F565B),
    faint: Color(0xFF6F666B),
    inkLogo: Color(0xFF1D1A1C),
    pink: Color(0xFFE0679F),
    pinkText: Color(0xFFA8336B),
    pinkSoft: Color.fromRGBO(224, 103, 159, 0.12),
    onPink: Color(0xFF1D1A1C),
    warnBg: Color(0xFFFDF3E1),
    warnLine: Color(0xFFECD29F),
    warnText: Color(0xFF5F420F),
    warnIcon: Color(0xFFB7791F),
    track: Color(0xFFECE5E1),
    disabled: Color(0xFFB9B0B4),
    danger: Color(0xFFB3261E),
  );

  static const dark = PostitTokens(
    bg: Color(0xFF121012),
    surface: Color(0xFF1B181A),
    raised: Color(0xFF242022),
    line: Color(0xFF2F2A2D),
    text: Color(0xFFF4EFF1),
    mutedText: Color(0xFFAEA4A9),
    faint: Color(0xFF948B90),
    inkLogo: Color(0xFFF4EFF1),
    pink: Color(0xFFE0679F),
    pinkText: Color(0xFFEC8BB7),
    pinkSoft: Color.fromRGBO(224, 103, 159, 0.14),
    onPink: Color(0xFF1D1A1C),
    warnBg: Color(0xFF2A2112),
    warnLine: Color(0xFF5C4518),
    warnText: Color(0xFFF2D9A8),
    warnIcon: Color(0xFFE8B04A),
    track: Color(0xFF2F2A2D),
    disabled: Color(0xFF6D6468),
    danger: Color(0xFFF2827A),
  );

  @override
  PostitTokens copyWith() => this;

  /// Themes switch instantly (as on the web), so there is nothing to interpolate.
  @override
  PostitTokens lerp(ThemeExtension<PostitTokens>? other, double t) => other is PostitTokens && t >= 0.5 ? other : this;
}

extension PostitTokensX on BuildContext {
  PostitTokens get tokens => Theme.of(this).extension<PostitTokens>() ?? PostitTokens.light;
}
