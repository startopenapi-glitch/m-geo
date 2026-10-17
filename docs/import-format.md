# CNVP JSON 交换格式


This is a **documented JSON exchange format**, not permission to read a live CNVP database or a claim that an existing CNVP exporter already emits it. A source exporter must transform its five business tables into this shape. It must exclude passwords, API keys, sessions, model settings and unrelated knowledge data.

```json
{
  "schemaVersion": 1,
  "brandName": "与当前品牌完全相同",
  "products": [],
  "questions": [],
  "drafts": [],
  "publications": [],
  "observations": []
}
```

Call `import.cnvp` with selected `brandId`, stable bounded `sourceInstance`, JSON-string `content`, and `confirm:true`. Limit 2 MiB; up to 1,000 products and 5,000 records in each dependent array. Unknown envelope/record/fact fields are rejected to avoid importing unrelated credentials/configuration. All five arrays are required.

Records use the corresponding camelCase shared type fields, with these explicit adaptations:

| Class | Exchange fields |
| --- | --- |
| Product | Legacy string `id`; `name`, `sku`, `category`, `officialUrl`, `sourceText`, `privateNotes`, `facts`, positive `revision`, optional nullable `confirmedAt`, optional `updatedAt` and `confirmationSnapshot`. `facts` follows `Fact` exactly, including optional nullable `validUntil`. Category/link/raw text/notes may be absent and default empty. |
| Question | Legacy string `id`, legacy `productId`; `text`, `intent`, `origin`, `source`, `branded`, `confirmed`. |
| Draft | Legacy string `id`, legacy `productId`, nullable legacy `questionId`; `kind`, `title`, exact `body`, positive `sourceRevision` and `revision`, `generationMode`, nullable `reviewedAt`, `createdAt`. Optional `sourceSnapshot`, `reviewSnapshot`. |
| Publication | Legacy string `id`, legacy `draftId`; positive `draftRevision`, `channel`, `url`, `publishedAt`, `note`, **exact** `contentSnapshot`. Optional lowercase `contentSnapshotSha256` validates SHA-256 of original UTF-8 snapshot bytes. |
| Observation | Legacy string `id`, legacy `productId` and `questionId`; original `querySnapshot` and `branded`, plus platform/surface/phase/modelLabel/searchMode/testedAt/answer/mentioned/accuracy/citedUrl/evidenceNote. Original query and cohort are preserved even if the current question differs or is unconfirmed. |

Every row may additionally include `brandName` (must match envelope/current brand), `ownerLabel`, and `reviewerLabel` (historical display labels only; no accounts/permissions are created). Do not include native `brandId` in records; the selected brand is checked and assigned natively. Legacy IDs may be arbitrary nonempty strings up to 200 characters; newly created native IDs are UUIDs. Datetimes are RFC 3339, fact expiry dates YYYY-MM-DD UTC.

Stable `(sourceInstance,kind,oldId)` mappings prevent repeated import. Repeated records are skipped; mapping reuse across another brand is rejected. Same-brand SKU collisions skip the product **and its historical descendants**; existing customer facts and their related content are never mixed with imported historical content. The response reports total created/skipped records; no individual conflict list is exposed by this contract.

Import preserves product/draft revision numbers, exact publication snapshot bytes, original observation snapshots/cohorts, and source/approval provenance. **Active imported product confirmation and draft review are intentionally cleared for revalidation.** The previous exact `confirmedAt`/`reviewedAt` JSON values, bounded owner/reviewer labels, supplied confirmation/source/review snapshot JSON and `revalidationStatus:"pending"` remain in `m_migration_history`. Snapshots are bounded to 300 KB each and reject structural credential fields recursively. Active shared types expose null approval status, so existing pending-confirmation/review UI applies; historical metadata is retained natively for audit and is not an additional frontend query endpoint.

A malformed late row, missing association, cross-brand reference, mismatching snapshot hash or duplicate legacy ID rejects the entire import without partial writes. Import is local only and never grants approval, publication success, AI citation or commercial benefits.

