## Email listing critical notifications not yet emailed

alert-subject = { $count ->
    [one] هناك أمر يحتاج إلى انتباهك في postit
    [two] هناك أمران يحتاجان إلى انتباهك في postit
    [few] { $count } أمور تحتاج إلى انتباهك في postit
    [many] { $count } أمرًا يحتاج إلى انتباهك في postit
   *[other] { $count } أمر يحتاج إلى انتباهك في postit
}
alert-intro = { $count ->
    [one] هذا الأمر يحتاج إلى انتباهك:
    [two] هذان الأمران يحتاجان إلى انتباهك:
   *[other] هذه الأمور تحتاج إلى انتباهك:
}
alert-open = افتح postit
alert-footer = وصلتك هذه الرسالة لأن إشعارات البريد الإلكتروني مفعّلة في إعدادات postit.

## One line per notification kind

notice-delivery_dead_lettered = تعذّر نشر منشور على { $account } في { $platform } بعد عدة محاولات.
notice-delivery_failed = فشل نشر منشور على { $account } في { $platform }.
notice-delivery_outcome_unknown = لم يتمكن postit من التأكد من نشر منشور على { $account } في { $platform }.
notice-delivery_parked = منشور على { $account } في { $platform } معلّق: { $reason }.
notice-delivery_stale = منشور على { $account } في { $platform } معلّق منذ مدة طويلة ويحتاج إليك: { $reason }.
notice-job_partial = { $count ->
    [one] نُشر منشور، لكن النشر فشل على حساب واحد من حساباته.
    [two] نُشر منشور، لكن النشر فشل على حسابين من حساباته.
    [few] نُشر منشور، لكن النشر فشل على { $count } حسابات من حساباته.
    [many] نُشر منشور، لكن النشر فشل على { $count } حسابًا من حساباته.
   *[other] نُشر منشور، لكن النشر فشل على { $count } حساب من حساباته.
}
notice-account_needs_reconnect = يجب إعادة ربط { $account } في { $platform }.
notice-account_revoked_by_platform = ألغت { $platform } وصول postit إلى { $account }.
notice-reconnect_by_passed = انقضى موعد إعادة ربط { $account } في { $platform }.
notice-recurrence_skipped = تخطّى منشور متكرر إحدى مرّاته: { $reason }.
notice-delivery_published = نُشر منشور على { $account } في { $platform }.
notice-reconnect_reminder = أعد ربط { $account } في { $platform } قريبًا لمواصلة النشر.
notice-delegation_changed = تغيّر الوصول المشترك بينك وبين { $name }.
notice-new_comments = { $count ->
    [one] لدى { $account } في { $platform } تعليق جديد.
    [two] لدى { $account } في { $platform } تعليقان جديدان.
    [few] لدى { $account } في { $platform } { $count } تعليقات جديدة.
    [many] لدى { $account } في { $platform } { $count } تعليقًا جديدًا.
   *[other] لدى { $account } في { $platform } { $count } تعليق جديد.
}
notice-user_pending = مستخدم جديد ينتظر الموافقة.

## Why a post is on hold

park-reason-owner_disabled = حساب مالك مساحة العمل معطّل
park-reason-account_needs_reconnect = يجب إعادة ربط الحساب
park-reason-platform_unavailable = المنصة غير متاحة
park-reason-platform_disabled = أوقف أحد المسؤولين المنصة
park-reason-live_publish_disabled = النشر الفعلي متوقف
park-reason-quota_exceeded = بلغ النشر الحد المسموح به في المنصة
park-reason-outcome_unresolvable = لا يستطيع postit معرفة ما إذا نُشر، لذا عليك أن تقرر
park-reason-unknown = هناك ما يمنعه خارج postit

## Why a recurring post skipped an occurrence

skip-reason-ai_provider_missing = لم يُعدّ مزوّد ذكاء اصطناعي لإعادة الصياغة
skip-reason-rewrite_invalid = لم تكن إعادة الصياغة بالذكاء الاصطناعي صالحة للاستخدام
skip-reason-job_active = المرة السابقة لا تزال قيد النشر

## Email with the daily digest of new comments

digest-subject = { $count ->
    [one] لديك تعليق جديد واحد على postit
    [two] لديك تعليقان جديدان على postit
    [few] لديك { $count } تعليقات جديدة على postit
    [many] لديك { $count } تعليقًا جديدًا على postit
   *[other] لديك { $count } تعليق جديد على postit
}
digest-intro = { $count ->
    [one] هذا هو التعليق الجديد منذ آخر ملخص:
    [two] هذان هما التعليقان الجديدان منذ آخر ملخص:
   *[other] هذه هي التعليقات الجديدة منذ آخر ملخص:
}
digest-open = افتح postit
digest-footer = وصلتك هذه الرسالة لأن الملخص اليومي للتعليقات مفعّل في إعدادات postit.

## Closing line of a long list

list-more = ...و{ $more } أخرى.

## Push notification titles, one per kind; the body repeats the kind's line above

push-title-delivery_dead_lettered = لم يُنشر المنشور
push-title-delivery_failed = فشل النشر
push-title-delivery_outcome_unknown = حالة المنشور غير معروفة
push-title-delivery_parked = منشور معلّق
push-title-delivery_stale = منشور يحتاج إليك
push-title-job_partial = نُشر المنشور جزئيًا
push-title-account_needs_reconnect = أعد ربط حساب
push-title-account_revoked_by_platform = أُلغي الوصول إلى حساب
push-title-reconnect_by_passed = انقضى موعد إعادة الربط
push-title-recurrence_skipped = تخطّي منشور متكرر
push-title-delivery_published = نُشر المنشور
push-title-reconnect_reminder = أعد الربط قريبًا
push-title-delegation_changed = تغيّر الوصول المشترك
push-title-new_comments = تعليقات جديدة
push-title-user_pending = مستخدم جديد بانتظار الموافقة

push-body-delivery_dead_lettered = { notice-delivery_dead_lettered }
push-body-delivery_failed = { notice-delivery_failed }
push-body-delivery_outcome_unknown = { notice-delivery_outcome_unknown }
push-body-delivery_parked = { notice-delivery_parked }
push-body-delivery_stale = { notice-delivery_stale }
push-body-job_partial = { notice-job_partial }
push-body-account_needs_reconnect = { notice-account_needs_reconnect }
push-body-account_revoked_by_platform = { notice-account_revoked_by_platform }
push-body-reconnect_by_passed = { notice-reconnect_by_passed }
push-body-recurrence_skipped = { notice-recurrence_skipped }
push-body-delivery_published = { notice-delivery_published }
push-body-reconnect_reminder = { notice-reconnect_reminder }
push-body-delegation_changed = { notice-delegation_changed }
push-body-new_comments = { notice-new_comments }
push-body-user_pending = { notice-user_pending }
