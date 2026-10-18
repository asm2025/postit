## ملخص الحسابات التي تنتظر الموافقة للمسؤولين

pending-subject = { $count ->
    [zero] لا توجد حسابات في postit تنتظر الموافقة
    [one] حساب واحد في postit ينتظر الموافقة
    [two] حسابان في postit ينتظران الموافقة
    [few] { $count } حسابات في postit تنتظر الموافقة
    [many] { $count } حسابًا في postit تنتظر الموافقة
   *[other] { $count } حساب في postit تنتظر الموافقة
}
pending-intro = { $count ->
    [one] هذا الحساب ينتظر الموافقة في postit:
    [two] هذان الحسابان ينتظران الموافقة في postit:
   *[other] هذه الحسابات تنتظر الموافقة في postit:
}
pending-more = ...و{ $more } أخرى.
pending-review = مراجعتها
pending-footer = وصلتك هذه الرسالة لأنك مسؤول في postit.

## إشعار الموافقة على الحساب

approved-subject = تمت الموافقة على حسابك في postit
approved-greeting = مرحبًا { $name }،
approved-body = وافق أحد المسؤولين على حسابك في postit.
approved-sign-in = سجّل الدخول الآن
