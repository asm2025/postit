import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:postit/ui/theme/tokens.dart';

/// The ink color baked into `postit-f.svg`; swapped for the theme's ink color so the
/// wordmark reads on dark backgrounds while the pink mark keeps its own color.
const _wordmarkInk = Color(0xFF1D1A1C);

class _InkMapper extends ColorMapper {
  const _InkMapper(this.ink);

  final Color ink;

  @override
  Color substitute(String? id, String elementName, String attributeName, Color color) =>
      color == _wordmarkInk ? ink : color;
}

/// The full wordmark in the theme's ink color.
class PostitWordmark extends StatelessWidget {
  const PostitWordmark({super.key, this.height = 28});
  final double height;

  @override
  Widget build(BuildContext context) => SvgPicture.asset(
    'assets/logo/postit-f.svg',
    height: height,
    colorMapper: _InkMapper(context.tokens.inkLogo),
    semanticsLabel: 'postit',
  );
}

/// The square pink badge.
class PostitBadge extends StatelessWidget {
  const PostitBadge({super.key, this.size = 56});
  final double size;

  @override
  Widget build(BuildContext context) =>
      SvgPicture.asset('assets/logo/postit-sp.svg', width: size, height: size, semanticsLabel: 'postit');
}
