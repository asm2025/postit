# B4 media: deferred issues

## The generated mobile client cannot send file bytes

`cargo xtask openapi` generates `mobile/lib/api/clients/media_api.dart` from the spec. The routes that
take raw bytes (`POST /media`, `PUT /media/uploads/{id}`) are described as
`application/octet-stream` with a string schema, so the generator (retrofit) types the body as `String`
and warns that it needs `toJson()`. Those two methods cannot carry a file, and the file download
routes return no typed body.

Why it cannot be fixed now: the generator has no binary-body type for an OpenAPI 3.1 byte stream, and
the mobile upload is built in D6, which needs a streaming client with progress and resume anyway.

Trigger: D6 (mobile media UI). It writes a small hand-written streaming client for the two upload
routes and the content routes on top of Dio, and leaves the generated methods unused.

## Files of a deleted user are reclaimed by the garbage collector, not at deletion

**Resolved in D1 (Task 13, branch `d1-publishing-notifications`).** `postit_media::MediaDeletionStep`
(`media`) removes the user's whole key tree (`{root}/{owner}/`, originals, thumbnails, and the bytes of
unfinished uploads) through `FsMediaStore::remove_owner_files`; a missing directory means done. The
server registers it after the `publishing` and `oauth` steps, so no delivery still reads a file.
Upload chunks in `.parts` are named by a fresh ID per request, not by owner: the request removes its
own and `media_gc` sweeps what a crashed request left.

`delete_user` cascades the user's `media_assets` rows, but their files stay on disk until the next
`media_gc` run (up to a day, then only after `media.upload_session_ttl`).

Why it cannot be fixed now: the explicit deletion step belongs to the account-deletion job that D1
adds with the publishing engine; B4 has no job to hang it on, and the plan assigns it to D1.

Trigger: D1. It adds a step to the user-deletion job that lists the user's keys before the cascade and
removes them.
