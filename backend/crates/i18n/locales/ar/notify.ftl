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
