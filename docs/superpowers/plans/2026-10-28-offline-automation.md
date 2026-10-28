# M-GEO Offline Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Execute the assigned task only, with RED/GREEN evidence. User explicitly requests parallel work; disjoint ownership below overrides serial execution guidance. Do not spawn additional agents. Root integrates and commits after review.

**Goal:** A durable zero-network fictional GEO workflow inside the existing desktop app.
**Architecture:** Rust/SQLite owns the workflow state machine, evidence and review gate. Pure Rust functions inspect HTML and construct simulated samples/local restructuring. React is a UI client; a native worker advances tasks independently of page visibility.
**Tech Stack:** Existing Tauri 2, Rust, SQLite, React/TypeScript; no new dependency.
**Spec:** docs/superpowers/specs/2026-10-28-offline-automation.md

## Global Constraints
- Simulated fixture only; no real model, network, platform login or publishing.
- Formal application and user data are not test targets; do not inspect secrets or key_ref contents.
- Every generated draft stays unreviewed until existing native draft.review is explicitly invoked.
- Preserve brand isolation, stale revision checks, transactionality, idempotence, backups and current fact gates.
- No new dependency; do not commit independently while parallel workers share this worktree.
- Use an isolated M-GEO worktree as the working directory.
- Root owns desktop.rs, App.tsx, version files, docs, packaging and integration scripts.

## Shared contract
Actions: offlineWorkflow.create/list/get/resume/pause/cancel/retry/step/export, offlineGeo.audit.
create input: {brandId,requestId,name,productId,questions:string[],repetitions:number,maxSamples:number,evidenceMode:'simulated'}.
mutations/get/export input: {brandId,taskId,expectedRevision} (get/list do not require revision).
list returns {tasks:Workflow[]}; get and mutations return Workflow (create may additionally have alreadyExists:boolean).
Workflow: {id,brandId,productId,productRevision,requestId,name,revision,status,questions,repetitions,draftId:string|null,createdAt,updatedAt,error:string|null,budget:{maxSamples,usedSamples,networkRequests:0,modelCalls:0,publications:0},nodes:Node[]}.
status: paused|running|awaiting_review|completed|failed|cancelled.
Node: {id,kind,dependsOn:string[],status:'pending'|'completed'|'blocked'|'failed',result:object|null,error:string|null}.
Node id and kind: baseline, rewrite, audit, review, followup; dependencies strictly previous node.
baseline result: {sampleCount,mentionCount,citationCount,samples:[{question,answer,citations:string[],mentioned:boolean,evidenceMode:'simulated',surface:'fixture'}],networkRequests:0,modelCalls:0}.
followup result has same fields and {mentionDelta:number,comparisonLabel:string}; compare with baseline.
rewrite result: {draftId,title}; audit result shape below. review result references approved draft revision.
export returns {files:[{path:string,content:string}]} using existing package conventions, with report JSON/Markdown and native reviewed draft export. Must reject unreviewed or stale candidate. No absolute paths/private data/settings in exports.
offlineGeo.audit input {brandId,html:string}; returns audit result without persistence.
Rust pure functions in core/offline_geo.rs:
`pub(super) fn simulate_monitor(questions:&[String], repetitions:u32, brand_name:&str, aliases:&[String], product_name:&str)->Result<Value,String>`
`pub(super) fn rewrite_body(original:&str)->Result<String,String>` (conservative structure only, retain all original lines and claims exactly)
`pub(super) fn audit_html(html:&str)->Result<Value,String>`
Audit result: {scope:'offline_html',checks:[{id,label,status:'pass'|'warning'|'fail',detail}],passed:number,warning:number,blocking:number,networkRequests:0}.
Native entry points root consumes: `core::tick_offline_workflows(&mut Connection)->Result<usize,String>`, `core::recover_offline_workflows(&mut Connection)->Result<usize,String>`.

## Review Focus
- Crash/repeated step must not duplicate candidate drafts; test idempotence and reopen recovery.
- Changed facts or cross-brand IDs must stop work/export; tests cover both.
- Unreviewed draft must remain blocked despite UI or forged result fields; native tests.
- HTML containing scripts/URLs/huge input/invalid JSON must not execute or fetch; bounded pure tests.
- Repeated UI clicks/stale async response must not create duplicates or apply to a new brand; UI tests.

### Task 1: Native workflow and schema lifecycle
**Owner files:** core/local_workflow.rs (+tests), core.rs module/routing, runtime.rs actions/schema, backup.rs, core/validation.rs, schema migration tests.
**Consumes:** pure functions above from Task 2. Can wire modules before files arrive; communicate any contract issue.
**Produces:** all workflow actions, tick/recovery public functions, schema9/backup roundtrip.
- [ ] Write failing tests for paused creation, neutral questions, budget rejection, idempotent request, independent brands, full fictional flow stopping at review, pause/cancel/retry, stale facts, duplicate step, reopen recovery and invalid backup state.
- [ ] Observe RED with existing locked Rust environment.
- [ ] Implement fixed graph only. Use native prepare/local_body/insert_draft or dispatch helpers inside the existing transaction; never nested handle transactions. rewrite_body retains native claims. Invoke current_draft/normal export for review and export gates.
- [ ] Register offlineGeo.audit as read-only, workflow mutations in appropriate validation/audit lists. tick must persist failure safely and not crash the worker; no externally supplied executable nodes/results.
- [ ] Migrate8→9 with snapshot, preserve old backup version acceptance; include new table in strict schema and content checks. Invalid workflow records rejected. Pause running workflows on reopen.
- [ ] Run targeted native tests; report RED/GREEN and regression evidence to root. No commit until root integration.

### Task 2: Offline monitoring, candidate structure and HTML checks
**Owner files:** core/offline_geo.rs and test module within it only. Do not edit core.rs or dependencies.
**Produces:** exact three pure signatures above.
- [ ] Write failing tests for 3 questions×2 repetitions, same baseline/followup results, local alias match, simulated-only sample provenance, zero network counters.
- [ ] Write failing tests retaining numeric facts/claims byte-for-byte in rewrite, rejecting empty/oversized inputs, safe HTML inspection (no DOM render), uppercase attributes, invalid JSON-LD, missing title/h1/description/body, pathological size.
- [ ] Implement simple deterministic fictional answer fixtures (explicit Chinese simulated label), never live API. URLs if any only example.invalid and labeled fictitious. No fabricated observed improvement.
- [ ] Implement conservative local structure rewrite and bounded HTML checks, reusing existing helpers where appropriate. HTML checks are hints, not external trust or ranking metrics.
- [ ] Verify RED/GREEN and report; shared module registration performed by Task 1.

### Task 3: Local automation page
**Owner files:** src/offlineWorkflow.ts, src/pages/OfflineWorkflowWorkbench.tsx, their tests only. Do not edit App.tsx/styles/dependencies.
**Consumes:** exact shared contract. Page props {brand:Brand,data:WorkspaceSnapshot,refresh:()=>Promise<void>,onNavigate?:(section:'content')=>void,onBusyChange?:(busy:boolean)=>void}.
- [ ] Write failing UI tests for create paused with stable request ID, explicit start, polling state without calling step/realAI/network, review blocker + link to existing Content, pause/resume/cancel/retry, export saved/cancel states and bounded offline HTML check.
- [ ] Build page with confirmed product selector, editable neutral questions (1 per line), repetitions and sample budget; clear zero-model/zero-network simulated labels. Default 3 neutral generic buying questions, repetitions1,maxSamples60.
- [ ] Render task status, graph progress, sample answers, audit checks, comparison labels and operation feedback; results are text, never dangerouslySetInnerHTML.
- [ ] Poll list/get while mounted for native worker progress, clean up stale requests; lock brand/navigation during mutations or export dialog. Export via file.exportOfflineWorkflow (root wires).
- [ ] Run focused Vitest and typecheck; no commit. Report gaps in contract immediately.

### Task 4: Integration, native worker and delivery (root)
**Owner files:** App.tsx and its tests, desktop.rs, package/Cargo/tauri versions, acceptance scripts/docs.
- [ ] Add nav entry and native worker tests/fixtures before wiring. Worker advances persisted running tasks while app runs, no page interval dependency, recovery pauses running tasks after restart.
- [ ] Add native export save/cancel path with bounded package construction and existing atomic writer.
- [ ] Bump candidate to0.20.0; build isolated Bundle ID com.mantou.mgeo.offline20261028, never overwrite installed App.
- [ ] Independent review of each module, fix/regressions then full frontend/Rust/typecheck/build/fmt gates.
- [ ] Fictional integration checks including migration8→9, encrypted backup/restore, malformed records, draft review/export, restart and pending work. Native UI check start/review/export/reopen/minimum window where available.
- [ ] Re-read formal counts/hash/non-sensitive model state and installed executable; compare baseline. Produce source/candidate/DMG/report and local commit; no push/release/publication.
