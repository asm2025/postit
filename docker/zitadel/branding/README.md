# Zitadel branding assets

`Onest.ttf` is the font `cargo xtask zitadel-bootstrap` uploads to Zitadel's label policy
(the login page and, where it applies, identity mail).

- Source: https://github.com/google/fonts/tree/main/ofl/onest (`Onest[wght].ttf`, saved here
  without the brackets). google/fonts ships Onest only as a variable font; its default
  instance is Regular.
- License: SIL Open Font License 1.1, see `OFL.txt` (copied from the same directory).
- Size: under Zitadel's 512 KB limit for font uploads.

The logo is the full wordmark `web/public/postit-f.svg`, read by the bootstrap at run time and
not copied here. The bootstrap uploads it as the light logo and, with the "post" ink
(`#1d1a1c`) swapped for the dark theme's font color, as the dark logo, so the wordmark reads
on both login themes.

Each run downloads the logos and font Zitadel serves and uploads only the ones whose bytes
differ from the local files, so a changed logo or font is applied by re-running
`cargo xtask zitadel-bootstrap`.
