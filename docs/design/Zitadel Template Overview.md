### Zitadel Template Overview

Zitadel handles email templating using Go HTML templates (`html/template`) and allows customization at the Instance or Organization level under **Settings > Notification / Mail Templates** (or via the Zitadel Management API).

Zitadel's email delivery distinguishes two layers:

1. **The Base Template**: The structural HTML envelope with the header, background, styling, container, and footer. It wraps every notification via the `{{ .Content }}` placeholder.
2. **The Message Content Templates**: Specific notification texts, specifically:
   - **Verify Email (Code)**: Sent during registration or email change when code verification is used (`{{ .Code }}`).
   - **Verify Email (URL)**: Sent when link verification is used (`{{ .URL }}`).

------

### Brand Assets & Style Variables

Extracted from the **postit** assets:

- **Brand Pink (Accent / CTA):** `#df528a` (approximated from the accent marks in the logo)
- **Dark Neutral (Primary Text / Brand):** `#1f1e24`
- **Muted Gray (Borders / Secondary Text):** `#6b7280` / `#f3f4f6`
- **Background / Container:** `#f8fafc` outer, `#ffffff` card container
- **Logo Reference:** In production, host the uploaded `postit` logo (e.g. `postit.png` / `postit-f.png`) on your public asset domain and replace `{{ .LogoURL }}` if not configured directly in Zitadel Console under **Appearance > Assets**.

------

### 1. Base Master Template

This wraps all outgoing Zitadel emails. Zitadel injects the specific message into `{{ .Content }}`.

```html
<!DOCTYPE html>
<html lang="en" dir="auto">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="X-UA-Compatible" content="IE=edge">
  <title>postit</title>
  <style>
    body, table, td, a { -webkit-text-size-adjust: 100%; -ms-text-size-adjust: 100%; }
    table, td { mso-table-lspace: 0pt; mso-table-rspace: 0pt; }
    img { -ms-interpolation-mode: bicubic; border: 0; outline: none; text-decoration: none; }
    body {
      margin: 0;
      padding: 0;
      width: 100% !important;
      min-width: 100%;
      background-color: #f8fafc;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
      color: #1f1e24;
    }
    .wrapper {
      width: 100%;
      table-layout: fixed;
      background-color: #f8fafc;
      padding-top: 40px;
      padding-bottom: 40px;
    }
    .main-card {
      background-color: #ffffff;
      margin: 0 auto;
      width: 100%;
      max-width: 560px;
      border-radius: 12px;
      border: 1px solid #e2e8f0;
      overflow: hidden;
      box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.04);
    }
    .header {
      padding: 32px 40px 24px 40px;
      text-align: left;
    }
    .content-body {
      padding: 0 40px 32px 40px;
      font-size: 15px;
      line-height: 1.6;
      color: #334155;
    }
    .btn {
      display: inline-block;
      padding: 12px 28px;
      font-size: 15px;
      font-weight: 600;
      color: #ffffff !important;
      background-color: #df528a;
      border-radius: 6px;
      text-decoration: none;
      text-align: center;
      margin: 20px 0;
    }
    .code-box {
      display: inline-block;
      padding: 14px 28px;
      font-size: 26px;
      letter-spacing: 6px;
      font-weight: 700;
      font-family: 'Courier New', Courier, monospace;
      color: #1f1e24;
      background-color: #fdf2f6;
      border: 1px solid #f9d8e5;
      border-radius: 8px;
      margin: 20px 0;
    }
    .footer {
      padding: 24px 40px 32px 40px;
      border-top: 1px solid #f1f5f9;
      font-size: 12px;
      line-height: 1.5;
      color: #94a3b8;
    }
    @media only screen and (max-width: 600px) {
      .main-card { border-radius: 0; border: none; }
      .header, .content-body, .footer { padding-left: 20px !important; padding-right: 20px !important; }
    }
  </style>
</head>
<body>
  <div class="wrapper">
    <table role="presentation" border="0" cellpadding="0" cellspacing="0" width="100%">
      <tr>
        <td align="center">
          <table role="presentation" border="0" cellpadding="0" cellspacing="0" class="main-card">
            <!-- Header / Brand -->
            <tr>
              <td class="header">
                {{ if .LogoURL }}
                  <img src="{{ .LogoURL }}" alt="postit" width="120" style="display:block; max-width:120px; height:auto;">
                {{ else }}
                  <span style="font-size: 24px; font-weight: 800; color: #1f1e24; letter-spacing: -0.5px;">post<span style="color: #df528a;">it</span></span>
                {{ end }}
              </td>
            </tr>
            <!-- Dynamic Content Injected by Zitadel -->
            <tr>
              <td class="content-body">
                {{ .Content }}
              </td>
            </tr>
            <!-- Footer -->
            <tr>
              <td class="footer">
                <p style="margin: 0 0 8px 0;">This email was sent by <strong>postit</strong> to confirm your email address.</p>
                <p style="margin: 0;">If you did not initiate this request, you can safely ignore this email.</p>
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </div>
</body>
</html>
```

------

### 2. Verify Email: Magic Link (`Verify Email URL`)

In Zitadel Console under **Notification Templates > Verify Email (URL)**:

```html
<h2 style="font-size: 20px; font-weight: 700; color: #1f1e24; margin: 0 0 16px 0;">
  Verify your email address
</h2>

<p style="margin: 0 0 16px 0;">
  Hello {{ if .PreferredLoginName }}{{ .PreferredLoginName }}{{ else }}there{{ end }},
</p>

<p style="margin: 0 0 16px 0;">
  Thank you for creating an account on <strong>postit</strong>. Click the button below to confirm your email address and activate your account:
</p>

<table role="presentation" border="0" cellpadding="0" cellspacing="0">
  <tr>
    <td align="center">
      <a href="{{ .URL }}" class="btn" target="_blank" rel="noopener noreferrer">
        Confirm Email Address
      </a>
    </td>
  </tr>
</table>

<p style="margin: 16px 0 0 0; font-size: 13px; color: #64748b;">
  If the button does not work, copy and paste this link into your browser:
</p>
<p style="margin: 4px 0 0 0; font-size: 13px; word-break: break-all;">
  <a href="{{ .URL }}" style="color: #df528a; text-decoration: underline;">{{ .URL }}</a>
</p>
```

------

### 3. Verify Email: One-Time Code (`Verify Email Code`)

In Zitadel Console under **Notification Templates > Verify Email (Code)**:

```html
<h2 style="font-size: 20px; font-weight: 700; color: #1f1e24; margin: 0 0 16px 0;">
  Verify your email address
</h2>

<p style="margin: 0 0 16px 0;">
  Hello {{ if .PreferredLoginName }}{{ .PreferredLoginName }}{{ else }}there{{ end }},
</p>

<p style="margin: 0 0 16px 0;">
  Enter the following verification code on the confirmation screen to verify your email address:
</p>

<div style="text-align: center;">
  <div class="code-box">
    {{ .Code }}
  </div>
</div>

<p style="margin: 16px 0 0 0; font-size: 13px; color: #64748b;">
  This verification code will expire shortly. Do not share this code with anyone.
</p>
```

------

### Configuration & Deployment Notes

1. **Zitadel Go-Template Context Variables**:
   - `{{ .Content }}`: Zitadel’s body placeholder inside the base template.
   - `{{ .URL }}`: Verification action link.
   - `{{ .Code }}`: Numeric/alphanumeric one-time token.
   - `{{ .PreferredLoginName }}`: The user's username or email prefix.
   - `{{ .DisplayName }}`: Full name if populated.
   - `{{ .LogoURL }}`: Asset URL configured in Zitadel's branding settings.
2. **Console vs. API/Bootstrap**:
   - In Zitadel Console: Navigate to **Default Settings** (instance level) or **Organization Settings** > **Notification Templates**.
   - Paste the Master Template in **Custom Template (Base HTML)**.
   - Paste the link or code snippets into **Verify Email (URL)** and **Verify Email (Code)** accordingly.
   - To automate this in your development environment, Zitadel’s management gRPC/REST endpoint `SetCustomMailTemplate` can be called from `cargo xtask zitadel-bootstrap` in `backend/`.