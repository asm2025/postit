import 'package:flutter/widgets.dart';
import 'package:go_router/go_router.dart';

class ShellScreen extends StatelessWidget {
  const ShellScreen({super.key, required this.shell});

  final StatefulNavigationShell shell;

  @override
  Widget build(BuildContext context) => shell;
}
