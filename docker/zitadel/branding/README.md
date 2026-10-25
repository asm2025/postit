# Zitadel branding assets

`Onest.ttf` is the font `cargo xtask zitadel-bootstrap` uploads to Zitadel's label policy
(the login page and, where it applies, identity mail).

- Source: https://github.com/google/fonts/tree/main/ofl/onest (`Onest[wght].ttf`, saved here
  without the brackets). google/fonts ships Onest only as a variable font; its default
  instance is Regular.
- License: SIL Open Font License 1.1, see `OFL.txt` (copied from the same directory).
- Size: under Zitadel's 512 KB limit for font uploads.

The logo is `web/public/postit.svg`, read by the bootstrap at run time and not copied here.

A changed logo or font is not re-uploaded automatically: the bootstrap uploads each only
while the label policy has none. To replace one, delete the asset in the Zitadel console
(Settings, Branding), then re-run `cargo xtask zitadel-bootstrap`.
