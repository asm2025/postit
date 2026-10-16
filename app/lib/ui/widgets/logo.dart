import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:postit/ui/theme/tokens.dart';

/// The full wordmark, tinted with the theme's ink color.
class PostitWordmark extends StatelessWidget {
  const PostitWordmark({super.key, this.height = 28});
  final double height;

  @override
  Widget build(BuildContext context) => SvgPicture.asset(
    'assets/logo/postit-f.svg',
    height: height,
    colorFilter: ColorFilter.mode(context.tokens.inkLogo, BlendMode.srcIn),
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
