// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/media_dto.dart';
import '../models/media_usage_dto.dart';
import '../models/page_media_dto.dart';
import '../models/upload_offset_dto.dart';
import '../models/upload_session_dto.dart';
import '../models/upload_session_request.dart';
import '../models/view_urls_dto.dart';

part 'media_api.g.dart';

@RestApi()
abstract class MediaApi {
  factory MediaApi(Dio dio, {String? baseUrl}) = _MediaApi;

  /// Lists the owner's media, newest first.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/media')
  Future<PageMediaDto> listMedia({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Uploads a whole file in one request. The body is the raw file and `Content-Length` is.
  /// required. The stored type is what the bytes sniff as, never the request's `Content-Type`.
  /// With an `Idempotency-Key`, a replay is recognized by the file name, `Content-Length`, and.
  /// `Content-Type` (the streamed body is not hashed).
  ///
  /// # Errors.
  ///
  /// 413 `payload_too_large` (kind limit or single-request limit) or `storage_quota_exceeded`.
  /// before any byte is stored (with `max_bytes` either way), 415 `unsupported_media_type`, 422.
  /// `validation_failed` for a bad name, a missing `Content-Length`, or a body that does not.
  /// match it, and 408 `request_timeout` when the body sits idle too long.
  ///
  /// [filename] - The file name, 1 to 255 characters. Its extension (`jpg`, `png`, `gif`, `webp`, `mp4`,.
  /// `m4v`, `mov`) picks the kind and so the size limit. Metadata only.
  @POST('/api/v1/media')
  Future<MediaDto> uploadMedia({
    @Query('filename') required String filename,
    @Body() required String body,
  });

  /// Starts a resumable upload. Send the file in chunks with `PUT /media/uploads/{id}`, ask.
  /// `HEAD` where it stands, and close it with `POST /media/uploads/{id}/complete`.
  ///
  /// # Errors.
  ///
  /// The refusals of the single-request upload that need no body: 413, 415, 422.
  @POST('/api/v1/media/uploads')
  Future<UploadSessionDto> startMediaUpload({
    @Body() required UploadSessionRequest body,
  });

  /// Appends a chunk. `Content-Range: bytes {start}-{end}/{total}` is required, `start` must be.
  /// the stored offset, and the body must be exactly `end - start + 1` bytes. After a dropped.
  /// connection, ask `HEAD` for the offset and resume there: bytes stored past it are cut off.
  ///
  /// # Errors.
  ///
  /// 409 `upload_offset_mismatch` (see `offset`), 413 over the chunk limit, 422.
  /// `validation_failed` (`field: content_range`), 404 for a missing or expired session, and 408.
  /// `request_timeout` when the body sits idle too long.
  ///
  /// [id] - Upload ID.
  ///
  /// [contentRange] - `bytes {start}-{end}/{total}`.
  @PUT('/api/v1/media/uploads/{id}')
  Future<UploadOffsetDto> appendMediaChunk({
    @Path('id') required String id,
    @Header('Content-Range') required String contentRange,
    @Body() required String body,
  });

  /// Where an upload stands: `Upload-Offset` (the next byte to send) and `Upload-Length` (the.
  /// declared size), with no body.
  ///
  /// # Errors.
  ///
  /// 404 for a missing or expired session.
  ///
  /// [id] - Upload ID.
  @HEAD('/api/v1/media/uploads/{id}')
  Future<void> mediaUploadStatus({
    @Path('id') required String id,
  });

  /// Closes an upload whose last byte arrived: checks the bytes, reads their size, and makes the.
  /// thumbnail. A file that is not an accepted image or video of the kind its name claims fails.
  /// the upload and removes it. Any other failure leaves the upload open, so the request can be.
  /// sent again. The check runs to its end even when this request times out; read the asset to.
  /// see how it went.
  ///
  /// # Errors.
  ///
  /// 409 `upload_offset_mismatch` (see `offset`) before the last byte, 415.
  /// `unsupported_media_type`, 404 for a missing or expired session.
  ///
  /// [id] - Upload ID.
  @POST('/api/v1/media/uploads/{id}/complete')
  Future<MediaDto> completeMediaUpload({
    @Path('id') required String id,
  });

  /// Storage used against the quota.
  ///
  /// # Errors.
  ///
  /// 403 without `view` access.
  @GET('/api/v1/media/usage')
  Future<MediaUsageDto> mediaUsage();

  /// One asset.
  ///
  /// # Errors.
  ///
  /// 404 for a missing asset.
  ///
  /// [id] - Media ID.
  @GET('/api/v1/media/{id}')
  Future<MediaDto> getMedia({
    @Path('id') required String id,
  });

  /// Deletes an asset and its files.
  ///
  /// # Errors.
  ///
  /// 409 `media_in_use` while a post or an unfinished job uses it.
  ///
  /// [id] - Media ID.
  @DELETE('/api/v1/media/{id}')
  Future<void> deleteMedia({
    @Path('id') required String id,
  });

  /// The file, with `Range` and conditional-request support. The type is what the bytes sniffed.
  /// as; the response is sandboxed and never sniffed.
  ///
  /// # Errors.
  ///
  /// 404 for a missing or unfinished asset, 416 for a range past the end.
  ///
  /// [id] - Media ID.
  @GET('/api/v1/media/{id}/content')
  Future<void> getMediaContent({
    @Path('id') required String id,
  });

  /// The JPEG thumbnail, longest side 480 px.
  ///
  /// # Errors.
  ///
  /// 404 for a missing asset or one without a thumbnail.
  ///
  /// [id] - Media ID.
  @GET('/api/v1/media/{id}/thumbnail')
  Future<void> getMediaThumbnail({
    @Path('id') required String id,
  });

  /// Short-lived signed URLs for showing the asset without a bearer token, for example in an.
  /// `<img>` tag.
  ///
  /// # Errors.
  ///
  /// 404 for a missing or unfinished asset.
  ///
  /// [id] - Media ID.
  @GET('/api/v1/media/{id}/view-url')
  Future<ViewUrlsDto> getMediaViewUrl({
    @Path('id') required String id,
  });

  /// The file behind a signed URL from `GET /media/{id}/view-url`, with no bearer token. The.
  /// signature covers the asset and the expiry; anything wrong with it, or no signature at all,.
  /// is `403 forbidden`, with the same answer whether or not the asset exists.
  ///
  /// # Errors.
  ///
  /// 403 `forbidden` for a bad, missing, or expired signature; 404 when the asset is gone.
  ///
  /// [id] - Media ID.
  ///
  /// [exp] - Unix time the URL stops working.
  ///
  /// [sig] - The URL's signature.
  @GET('/public/media/{id}')
  Future<void> getPublicMedia({
    @Path('id') required String id,
    @Query('exp') String? exp,
    @Query('sig') String? sig,
  });

  /// The JPEG thumbnail behind a signed URL. It needs its own signature: the content URL's does.
  /// not work here.
  ///
  /// # Errors.
  ///
  /// 403 `forbidden` for a bad, missing, or expired signature; 404 when the asset is gone or has.
  /// no thumbnail.
  ///
  /// [id] - Media ID.
  ///
  /// [exp] - Unix time the URL stops working.
  ///
  /// [sig] - The URL's signature.
  @GET('/public/media/{id}/thumbnail')
  Future<void> getPublicMediaThumbnail({
    @Path('id') required String id,
    @Query('exp') String? exp,
    @Query('sig') String? sig,
  });
}
