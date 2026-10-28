# P0 Integration Rehearsal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. The user explicitly requested parallel subagents; separate owned files and root-only integration take precedence over the skill's serial default.

**Goal:** Implement the account-independent P0 capabilities as an isolated, persistent, strictly simulated end-to-end rehearsal in 0.21.0.

**Architecture:** Pure transport/request/site-fixture contracts, a native transactional task/ledger/CMS simulator, and a dedicated frontend. Root owns registration, schema migration, backup, export, version and integration. External side-effect transports are unavailable.

**Tech Stack:** Existing React/TypeScript, Rust/SQLite, Tauri; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-28-p0-integration-rehearsal.md`

## Global Constraints

- Work only in this isolated linked worktree on codex/mgeo-p0-offline-021. No main/push/PR/Release changes.
- Do not touch formal App/data/config/keychain, inspect secrets, run real models, access real accounts or publish.
- New tasks only accept fixture transport, HTTPS `.invalid` targets and simulated evidence. Counters for real side effects remain zero.
- Preserve native draft review; separate simulation publish approval binds current product/draft/target hash.
- No dependencies or unrelated restructuring. All model budgets are simulation units, never actual costs.
- Subagents own disjoint files and do not commit shared changes or spawn further agents. Root integrates and commits after reviews.

## Review Focus

- Restored/tampered tasks cannot fake real origins, approved versions, spent budget, receipts, or cross-brand references.
- Unknown publish outcome is reconciled using the original operation ID, never blindly reissued.
- Product/fact/draft/target changes invalidate use of old approval, including after a save dialog or background step.
- Restart/restore/global stop pauses execution; actions from stale frontend reads cannot mutate a newer revision.
- HTML comments/script/template/robots groups/redirect targets cannot produce false successful fixture readback.

## Shared Interfaces

Actions `p0Workflow.create/list/get/resume/pause/cancel/retry/step/approvePublish/reconcile/rollback/export`, `p0Workflow.previewMonitor`, `p0Workflow.auditFixture`, `p0Workflow.policyGet/policyUpdate/haltAll`.

All task actions include brandId; mutations include taskId+expectedRevision; create includes requestId(UUID),name,productId,question,mode,targetUrl,scenario,maxUnits,maxCalls,transport:'fixture',evidenceMode:'simulated'. Modes `natural_discovery|informed_qa`. Scenarios `success|rate_limit_once|publish_timeout|readback_mismatch|robots_blocked`. Limits: name1–100 chars, question1–500, task units/calls1–100, max100 tasks/brand, JSON task<=2MiB.

Task JSON: id,brandId,productId,productRevision,requestId,name,revision,mode,question,targetUrl,scenario,transport,evidenceMode,status,createdAt,updatedAt,draftId,approval,error,budget:{maxUnits,maxCalls,usedUnits,usedCalls},nodes:[{id,kind,status,result,error}],ledger:[operation records],remote,realEffects:{networkRequests:0,modelCalls:0,publications:0}. Node kinds fixed baseline,generate,audit,review,publish,readback,followup. Status paused,running,awaiting_review,awaiting_publish_approval,awaiting_reconcile,retry_wait,budget_blocked,completed,failed,cancelled. Node pending/completed/blocked/failed. `list` returns `{tasks:[full Task]}`; `get` returns Task; mutators return Task. Policy actions are brand-scoped for auditing but policy applies to whole workspace: revision,dailyMaxUnits,dailyMaxCalls,halted; `policyGet` also returns day,usedUnits,usedCalls (derived from ledger). Defaults100/100. policyUpdate requires expectedRevision and bounds1–10000. haltAll sets halted and pauses running tasks; resuming policy does not resume tasks.

`previewMonitor` input brandId,productId,mode,question; returns pure request contract. `auditFixture` accepts brandId plus fixture object. ApprovePublish requires confirm:true. Reconcile and rollback require confirm:true. Export returns `{files:[{path,content}],...}` compatible with existing site ZIP writer. Root owns `file.exportP0Workflow` native save dialog wrapper and stale recheck.

Pure module `core/p0_contracts.rs` exports pub(super):
- `monitor_request(mode:&str, question:&str, brand:&Value, product:&Value)->Result<Value>`
- `monitor_fixture(request:&Value)->Result<Value>`
- `site_fixture(url:&str,title:&str,body:&str)->Value`
- `audit_site_fixture(fixture:&Value)->Result<Value>`
- `capabilities()->Value`

Request result must include mode,question,contextIncluded,transport:'fixture',evidenceMode:'simulated',request:{prompt}, zero counters. Site fixture fields: url,finalUrl,statusCode,headers:{contentType,xRobotsTag},html,robotsTxt,sitemapXml,expectedText,transport:'fixture'. Site audit result includes scope:'fixture_site',checks:[{id,label,status:'pass|warning|fail',detail}],blocking,warning,passed,networkRequests:0. `site_fixture` uses `.invalid` host URL and renders escaped visible body; generated JSON-LD is visibly identified local simulated article data, never real evidence.

Native module `core/p0_workflow.rs` exports pub(super) TABLE_SQL, dispatch(c:&Connection,b:&str,a:&str,p:&Value)->Result<Value>, validate_database(c:&Connection)->Result<()>, tick(c:&mut Connection,recover:bool)->Result<usize>. Two tables named m_p0_workflows and m_p0_policy; no other tables. All JSON records strict key/type/size/relations validation. Root registers TABLE_SQL, schema10, dispatch, tick/recover, backup version checks and validation. B supplies any audit mapping requirements to root.

### Task 1: Pure request and site contracts (agent A)

Files: create core/p0_contracts.rs (inline tests), frontend p0Contracts.ts/test.ts only if needed; edit aiMonitoring.tsx and aiMonitoring.test.tsx only for accurate informed-test UI wording.

- [ ] First write failing tests for natural request containing no product data, informed facts, branded questions rejected, unsupported transport rejected, deterministic fixture identity, commented/script/template text not evidence, noindex/robots/canonical/sitemap/body mismatch and bounded inputs.
- [ ] Implement shared functions with existing parser utilities/quick-xml/url; do not add network. Request mode labels must distinguish API vs consumer natural results.
- [ ] Run focused Rust and frontend tests; report red/green and exact contracts to root. No changes to core.rs or central schema files.

### Task 2: Persistent engine, budgets and mock CMS (agent B)

Files: create core/p0_workflow.rs and core/p0_workflow_tests.rs. Root registers modules.

- [ ] Write failing workflow tests with fictional product fixtures. Test native review gate, independent publish approval, stale version, cross-brand, true transport injection and restoration tampering.
- [ ] Implement shared actions and table schema. Reuse prepare/local_body/insert_draft/current_draft; delete temporary AI preparations as existing local workflow does. No new real observations/publication records.
- [ ] Budget every fixture operation (unit cost1 except 429 costs0, attempted calls still count1); cap task+workspace UTC-day totals. Max3 attempts per operation, 429 retry available after at least1sec; unknown publish requires reconciliation with same operation ID. Do not charge/publish twice on reconcile.
- [ ] Mock CMS persists preview/published version/receipt and rollback (initial publication can return to unpublished); rendering and audit use Task1 contracts. Publish timeout records unknown outcome with existing remote result; reconciliation verifies it and only then advances.
- [ ] Tick serially under transaction, one step per task; review/approval/failure/cap/unknown states stop. Recover pauses running/retry-wait tasks. Global halt stops subsequent tasks/steps.
- [ ] Strict restore validation checks ledger-derived totals, statuses, references, receipt/approval hashes, realEffects zero, bounds. Export stable evidence with native reviewed draft recheck.
- [ ] Run focused tests and send contract refinements/report. Do not edit central registration/schema/backup/runtime/worker files.

### Task 3: User workflow UI (agent C)

Files: create p0Workflow.ts, pages/P0WorkflowWorkbench.tsx, pages/P0WorkflowWorkbench.test.tsx; edit App.tsx/navigation tests and styles.css for minimal additions.

- [ ] Write failing interaction tests for mode isolation, fixture-only message, create/resume/pause/review navigation/approval, unknown outcome requiring reconcile, budget/global halt, rollback and export cancel/saved feedback.
- [ ] Implement shared API contract. Keep original offline page. New nav AI测试→接入演练. Poll selected task while alive, guard brand switch/unmount/stale response/revision; on uncertain mutation re-read rather than auto-repeat.
- [ ] Show request preview, costs labelled模拟额度, actual calls0, step evidence, mock CMS readback, independent approval checkbox, no automatic content review. Call native file.exportP0Workflow; no JS download or actual network.
- [ ] Provide defaults: natural_discovery; target https://shop.example.invalid/products/demo; success; maxUnits10/maxCalls10. Allow canned fixture audit choices or inline bounded fixture JSON as appropriate, never open target URL.
- [ ] Responsive at960×640; scoped controls and scrollable outputs; no secrets/UI credential fields.
- [ ] Run targeted frontend tests/typecheck once contracts available, report exact results.

### Task 4: Root integration, migration and packaging

Files: core.rs, core/validation.rs, runtime.rs, backup.rs, offline_worker.rs, desktop.rs, BackupInspector/types as needed; version files and docs.

- [ ] Run clean baseline before parallel implementation; snapshot formal hashes and safe settings only.
- [ ] Register new actions/table modules and worker/recovery. Update schema9→10 with before-p0-v10 snapshot, strict old-version shape validation, backup/restore counts and active-task guard; add meaningful migration/backup/tamper tests.
- [ ] Native export dialog writes ZIP with audit files using pre/post dialog task+draft validation. Add adapter dispatch coverage to scripting driver.
- [ ] Integrate agents, independent task review then final branch review; fix findings and add regressions.
- [ ] Full cargo test --locked --quiet, cargo fmt -- --check, npm test, npm run typecheck, npm run build, isolated Tauri app build and signing/package verification.
- [ ] Isolated virtual product end-to-end plus native UI save/cancel/reopen/minimum window; no formal test writes. Compare formal data/config/App with preflight. Deliver exact candidate, verification report, unmet real-input list. Do not install formal/push/release.
