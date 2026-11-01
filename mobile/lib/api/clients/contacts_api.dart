// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'dart:convert';
import 'dart:io';

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/contact_dto.dart';
import '../models/contact_import_dto.dart';
import '../models/contact_list_dto.dart';
import '../models/contact_list_request.dart';
import '../models/contact_status_dto.dart';
import '../models/create_contact_request.dart';
import '../models/import_options.dart';
import '../models/page_contact_dto.dart';
import '../models/page_contact_list_dto.dart';
import '../models/page_suppression_dto.dart';
import '../models/update_contact_request.dart';

part 'contacts_api.g.dart';

@RestApi()
abstract class ContactsApi {
  factory ContactsApi(Dio dio, {String? baseUrl}) = _ContactsApi;

  /// The owner's contact lists, with their member counts.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/contact-lists')
  Future<PageContactListDto> listContactLists({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Creates an empty list. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` with `field` `name` for a blank name.
  @POST('/api/v1/contact-lists')
  Future<ContactListDto> createContactList({
    @Body() required ContactListRequest body,
  });

  /// Deletes a list; its contacts stay. Recorded as `contact_list_deleted`. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown list.
  ///
  /// [id] - List ID.
  @DELETE('/api/v1/contact-lists/{id}')
  Future<void> deleteContactList({
    @Path('id') required String id,
  });

  /// Renames a list. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown list; 422 `validation_failed` with `field` `name` for a blank name.
  ///
  /// [id] - List ID.
  @PATCH('/api/v1/contact-lists/{id}')
  Future<ContactListDto> renameContactList({
    @Path('id') required String id,
    @Body() required ContactListRequest body,
  });

  /// Adds a contact to a list; one already in it stays. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 when the list or the contact is not the owner's.
  ///
  /// [id] - List ID.
  ///
  /// [contactId] - Contact ID.
  @PUT('/api/v1/contact-lists/{id}/members/{contact_id}')
  Future<void> addContactListMember({
    @Path('id') required String id,
    @Path('contact_id') required String contactId,
  });

  /// Removes a contact from a list; one not in it changes nothing. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown list.
  ///
  /// [id] - List ID.
  ///
  /// [contactId] - Contact ID.
  @DELETE('/api/v1/contact-lists/{id}/members/{contact_id}')
  Future<void> removeContactListMember({
    @Path('id') required String id,
    @Path('contact_id') required String contactId,
  });

  /// The owner's contacts, oldest first, narrowed by list, tag, or status.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging or filters.
  ///
  /// [listId] - Only members of this list.
  ///
  /// [tag] - Only contacts with this tag.
  ///
  /// [status] - Only contacts with this status.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/contacts')
  Future<PageContactDto> listContacts({
    @Query('list_id') String? listId,
    @Query('tag') String? tag,
    @Query('status') ContactStatusDto? status,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Adds a contact with its consent record, or updates the contact that has the address.
  /// A new contact counts against the owner's contacts cap; an update never does. Both answer.
  /// 201 with `Location`, so a client cannot tell them apart by status. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 409 `contact_limit_exceeded` at the cap; 422 `validation_failed` with `field` (`email` or.
  /// `consent_source`).
  @POST('/api/v1/contacts')
  Future<ContactDto> createContact({
    @Body() required CreateContactRequest body,
  });

  /// The owner's contacts as a CSV download, optionally only one list's members, streamed. The.
  /// columns are `email`, `first_name`, `last_name`, `tags` (joined with `|`), `locale`,.
  /// `consent_source`, `consent_at`, `status`, then one per custom field. A cell a spreadsheet.
  /// would run as a formula starts with `'`; the import removes it. Recorded as.
  /// `contacts_exported` before the first byte. Owner-only.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown `list_id`.
  ///
  /// [listId] - Only members of this list.
  @GET('/api/v1/contacts/export')
  Future<String> exportContacts({
    @Query('list_id') String? listId,
  });

  /// Starts a CSV import. The body is `multipart/form-data` with an `options` part (JSON) and.
  /// then a `file` part. `options.attest_consent` must be `true`; `columns` maps CSV headers to.
  /// contact fields (`email` and `consent_source` required). The file is read as it arrives:.
  /// rows the file condemns are reported at once, the rest are applied by a background job, and.
  /// `GET /contacts/imports/{id}` shows progress. A replay with the same `Idempotency-Key`,.
  /// `options`, and `Content-Length` answers the first import. Owner-only.
  ///
  /// # Errors.
  ///
  /// 400 `consent_attestation_required`; 404 for an unknown `list_id`; 413 `csv_too_large`.
  /// past `contacts.csv_max_bytes` or `contacts.csv_max_rows`, or `payload_too_large` (with.
  /// `max_bytes`) for a declared length far beyond them; 422 `csv_invalid` (a mapped header.
  /// missing, or not UTF-8) or `validation_failed` (not multipart, parts out of order, bad.
  /// options, or `Idempotency-Key` without `Content-Length`).
  ///
  /// [idempotencyKey] - Replays the first response for the same options and length.
  ///
  /// [file] - The CSV file: UTF-8, comma or semicolon separated, with a header row.
  ///
  /// [options] - The import options, as JSON. Must come first.
  @MultiPart()
  @POST('/api/v1/contacts/import')
  Future<ContactImportDto> importContacts({
    @Part(name: 'file') required File file,
    @Part(name: 'options') required ImportOptions options,
    @Header('Idempotency-Key') String? idempotencyKey,
  });

  /// One import, with its live counts and the rows it rejected. Owner-only.
  ///
  /// # Errors.
  ///
  /// 404 when the owner has no such import.
  ///
  /// [id] - Import ID.
  @GET('/api/v1/contacts/imports/{id}')
  Future<ContactImportDto> getContactImport({
    @Path('id') required String id,
  });

  /// One contact.
  ///
  /// # Errors.
  ///
  /// 404 when the owner has no such contact.
  ///
  /// [id] - Contact ID.
  @GET('/api/v1/contacts/{id}')
  Future<ContactDto> getContact({
    @Path('id') required String id,
  });

  /// Deletes a contact. Its suppression entry, if any, stays. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 when the owner has no such contact.
  ///
  /// [id] - Contact ID.
  @DELETE('/api/v1/contacts/{id}')
  Future<void> deleteContact({
    @Path('id') required String id,
  });

  /// Changes a contact. The address never changes (an `email` member is refused). `status`.
  /// moves between `subscribed` and `unsubscribed` only; unsubscribing keeps the address.
  /// suppressed even if the contact is deleted and added again, and only the owner can.
  /// subscribe it again. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 403 `owner_only` when a delegate subscribes a contact the owner unsubscribed; 404 for an.
  /// unknown contact; 409 `status_not_settable` (`bounced` or `complained`, or the address.
  /// bounced or complained) or `suppressed` (subscribing an address that bounced, complained,.
  /// or opted out); 422 `validation_failed`.
  ///
  /// [id] - Contact ID.
  @PATCH('/api/v1/contacts/{id}')
  Future<ContactDto> updateContact({
    @Path('id') required String id,
    @Body() required UpdateContactRequest body,
  });

  /// The owner's suppression list: addresses that are never emailed. An entry shows its.
  /// contact's ID and address while a contact with that address exists.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/suppressions')
  Future<PageSuppressionDto> listSuppressions({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Removes a bounce from the suppression list, so the address can be emailed again; its.
  /// contact goes back to `subscribed` unless the owner had unsubscribed it. An opt-out or a.
  /// complaint can only be reversed by the contact. Recorded as `suppression_removed`.
  /// Owner-only.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown entry; 409 `suppression_not_removable` unless it is a bounce.
  ///
  /// [id] - Suppression entry ID.
  @DELETE('/api/v1/suppressions/{id}')
  Future<void> deleteSuppression({
    @Path('id') required String id,
  });
}
