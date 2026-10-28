# M- GEO implementation contract

All paths relative `apps/m-geo`. JSON property names camelCase. String IDs UUID. Optional values null/omitted as defined in `src/types.ts`. Errors return `Result<_,String>` with safe Chinese message, no raw DB/provider errors. Dates ISO 8601 UTC; fact validUntil YYYY-MM-DD UTC (end-of-date expiry).

Native single command `dispatch(action: String,payload: Value)->Result<Value,String>`. UI uses `call<T>(action,payload={})`. Controller routes core whitelist to `core::handle(&mut Connection, &str, Value)`; AI prefix to async `ai::handle(Arc<Mutex<Connection>>, &str, Value)`; file actions handled separately. Core and AI initializers called when database opens, no remote calls.

## Core actions (objects use src/types.ts)

- `brand.list` {} -> Brand[]
- `brand.create` {name} -> Brand
- `workspace.snapshot` {brandId} -> WorkspaceSnapshot {products,questions,drafts,publications,observations}; all records belong to brand. First version bounded 1000 products, 5000 dependent rows each, explicit truncation indication if applied rather than silent truncation.
- `product.save` {brandId,id?,expectedRevision?,name,sku,category,officialUrl,sourceText,privateNotes,facts:Fact[]} -> Product. update clears confirmedAt and increments revision. New UI facts start with `reviewStatus:'pending'`; every fact carries `reviewedAt` and `updatedAt`. Native code forces pending and clears reviewedAt when label/value/source/visibility/validUntil changes, even if the payload claims approval. Review-only changes preserve the content updatedAt.
- `product.confirm` {brandId,id,expectedRevision} -> Product. Every public fact must be approved, sourced and current. Pre-0.16 facts without review fields retain their historical product-confirmation compatibility and do not acquire fabricated review timestamps; any content change moves them into the explicit review registry.
- `question.save` {brandId,productId,text,intent,origin,source,branded,confirmed} -> Question (dedup by normalized text within product).
- `question.confirm` {brandId,id,confirmed} -> Question
- `question.suggest` {brandId,productId} -> Question[] (template suggestions never real search data)
- `draft.generateLocal` {brandId,productId,kind,questionId?} -> Draft. Local template generation records `claims: [{id,text,factId}]` for each public fact in the source snapshot. AI and legacy drafts start with an empty claim list and require explicit human mapping.
- `draft.save` {brandId,id,expectedRevision,title,body,claims} -> Draft (clears review and any prior evidence snapshots). Claim text must be an exact substring of the body at review; claim IDs are unique UUIDs and each fact ID is bounded. Native parsing strips client-supplied `evidence` instead of trusting it.
- `draft.review` {brandId,id,expectedRevision} -> Draft (checks product confirmation/version/expiry and at least one claim linked to an approved, current, public, sourced fact). Empty, unknown, unapproved, private, expired or non-body claims are rejected. Success freezes `evidence:{snapshotVersion:1,factId,label,value,source,validUntil,factReviewedAt,factUpdatedAt}` inside each claim. SQLite schema remains 5; pre-0.17 reviewed drafts without snapshots remain readable.
- `draft.export` {brandId,id,expectedRevision,format:'markdown'|'html'} -> {fileName,content} (revalidates review/evidence/current state and any frozen snapshot against current approved facts, escapes HTML without scripts, and appends the frozen public claim/fact/source evidence). Missing, mismatched or tampered snapshots block outward actions; private facts and local paths are not evidence sources.
- `publication.add` {brandId,draftId,expectedRevision,channel,url,publishedAt,note} -> Publication (review/current required, immutable snapshot)
- `observation.add` {brandId,productId,questionId,platform,surface,phase,modelLabel,searchMode,testedAt,answer,mentioned,accuracy,citedUrl,evidenceNote} -> Observation (confirmed question, brand enforced, query snapshot and branded from question, URL/choice validation)
- `import.csv` {brandId,content,confirm:true} -> {created,skipped}. exact headers 品牌,商品名称,SKU,类目,商品链接. All rows must match selected brand, atomically reject cross-brand/invalid. 200 rows/2MB.
- `import.cnvp` {brandId,sourceInstance,content,confirm:true} -> {created,skipped} where content is JSON export described by core agent in docs/report; no passwords/keys.

## Internal core actions (never exposed to frontend)

- `internal.ai.prepare` {brandId,productId,kind,questionId?} -> context {brandId,productId,kind,questionId,sourceRevision,sourceSnapshot,title,prompt}; only confirmed public unexpired facts, prompt must exclude raw/private text.
- `internal.ai.commit` {context:<above immutable object>,body} -> Draft. Transaction rechecks current product/brand/revision. AI worker additionally verifies model config version before call.
- `internal.ai.extract.prepare` handled by AI module: requires explicit consent and bounded selected user text; result candidate text only, never auto-updates product.

## AI actions

- `ai.settings.get` {} -> ModelSettings {baseUrl,model,version,hasKey}; defaults DeepSeek https://api.deepseek.com/v1 deepseek-chat, hasKey false.
- `ai.settings.save` {baseUrl,model,apiKey?} -> ModelSettings (missing/empty apiKey keeps existing; validation first; OS vault only)
- `ai.settings.clearKey` {} -> ModelSettings
- `ai.test` {consent:true} -> {ok:true,message} or safe error; no request just saving settings.
- `ai.start` {brandId,productId,kind,questionId?,consent:true} -> AiJob
- `ai.extract` {brandId,text,consent:true} -> AiJob, candidate result text only. No mutation to facts.
- `ai.jobs` {brandId} -> AiJob[]; `ai.cancel` {brandId,id} -> AiJob.
- AiJob {id,brandId,productId:string|null,kind,status:'queued'|'running'|'succeeded'|'failed'|'cancelled'|'interrupted',message,resultText:string|null,draftId:string|null,createdAt}. Credentials and full prompt not returned by jobs.

Model adapter uses no redirects, HTTPS URLs only, explicit 90s max, bounded response body and output30000chars. Credentials retrieved natively, no debug log. Persist tasks, mark old queued/running interrupted during initialization, no auto retry. Data/config version changes invalidate final save. Cancellation idempotent; no claim remote cancellation/refund.

## Controller-native actions

- `file.openText` {kind:'csv'|'document'|'cnvp'} -> null | {name,content}; user file dialog; UTF8/GB18030 and safe DOCX extraction. This alone sends nothing externally.
- `file.exportDraft` {brandId,id,expectedRevision,format} -> null | {saved:true}; native save dialog then revalidate revision before write, delegates draft.export.
- `file.saveTemplate` {} -> null | {saved:true}; CSV template.
- `backup.create` {password} -> null | {saved:true}; encrypted snapshot user save dialog.
- `backup.restore` {password,confirm:true} -> null | {restored:true}; open selected encrypted file, safety snapshot, atomic replace; keys excluded; UI clears all state and reloads brands after success.
- `app.info` {} -> {name,version,dataLocation,platform,releaseChannel:'internal',updateUrl:null}; do not invent public release URL.
- `app.openPlatform` {platform:'doubao'|'yuanbao'|'qianwen'|'baidu'} -> {opened:true}; native predefined HTTPS URLs only.
- `app.openSource` {} -> {opened:true}; local bundled license/source instructions, no misleading public source link.

## Cross-agent rules

No changes to shared interfaces/manifests without controller. Core owns core files; UI owns App/components/pages/styles; AI owns ai files. Root owns tests/helpers/scripts except owned tests. Each implementer writes their report to `verification/<role>-report.md`, including test commands, red/green evidence and limitations, returns short status. No subagents or commits by implementers; controller integrates. Do not ship fake data as persistence; test transport is explicit injection only.
