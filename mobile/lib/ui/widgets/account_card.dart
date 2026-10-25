import 'package:flutter/material.dart';
import 'package:postit/api/export.dart';
import 'package:postit/ui/labels.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/auto_text.dart';
import 'package:postit/ui/widgets/label_text.dart';

/// Name, email and role of the signed-in user.
class AccountCard extends StatelessWidget {
  const AccountCard({super.key, required this.me});
  final MeDto me;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(
          children: [
            CircleAvatar(
              backgroundColor: context.tokens.pinkSoft,
              foregroundColor: context.tokens.pinkText,
              child: Text(me.displayName.characters.first.toUpperCase()),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  AutoDirectionText(me.displayName, style: text.titleMedium),
                  if (me.email != null) AutoDirectionText(me.email!, style: text.bodySmall),
                ],
              ),
            ),
            LabelText(roleLabel(context, me.role)),
          ],
        ),
      ),
    );
  }
}
