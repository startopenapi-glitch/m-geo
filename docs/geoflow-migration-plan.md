# GEOFlow → M-GEO 全量能力迁移架构计划

> 状态：架构计划，不代表下列能力已经实现或验收。
>
> 目标分支：`feat/geoflow-capability-migration`。本文中的“新增”均指后续规划文件，不表示当前仓库已经存在这些文件。

## 1. 基线、范围与不可变产品边界

### 1.1 已核对基线

- 上游仓库：`geoflow-upstream`。
- 上游完整 commit：`084eb2d795984d5227a8a04fb6d608b9ffcb9f15`（短 SHA：`084eb2d`）。
- 上游 `version.json`：`3.2.0-beta.2`，tag 为 `v3.2.0-beta.2`。这是预览版，不把其能力说明当成生产验收证明。
- 上游 `NOTICE`、`composer.json` 和 `LICENSE`：GNU AGPL Version 3 only，即 `AGPL-3.0-only`；版权标识为 `Copyright 2026 Yao Jingang`。历史 Apache-2.0 部分及上游引用的 MIT 部分仍按各自许可处理。
- M-GEO 已声明 `AGPL-3.0-only`。移植、改写或跨语言重实现时，继续保留 GEOFlow / Yao Jingang 来源、版权和适用许可文本；同步维护根目录及应用目录的 `THIRD_PARTY_NOTICES.md`，不能以“改成 Rust”消除许可义务。
- M-GEO 当前为 Tauri 2 + React + Rust + SQLite，原生入口是白名单 `dispatch(action,payload)`。当前 `runtime.rs`、`backup.rs` 和 `core.rs` 的数据库版本均为 7；本次只读召回不新增表、不提升 schema。后续新增表必须同步升级初始化、校验、备份、恢复和升级快照逻辑。

能力来源以该 commit 的真实源码为准。`.agents/skills/geoflow/references/geoflow-current-capability-map.md` 和管理员指南只是索引；其中未覆盖但源码已存在的专题、知识事实、链接池、URL 变更和协调更新等能力也在本计划内。

### 1.2 “全量迁移”的完成定义

全量迁移指九类能力逐项具有可验证的 M-GEO 实现、明确的本地等价实现，或经批准的服务端实现及桌面端入口；不允许静默删项，也不允许只复制页面名称。

以下情况不算完成：

1. 仅写 ADR、占位页面、mock 数据或“后续支持”。
2. 用本地导出冒充远端发布、托管上线或平台提交成功。
3. 用 AI 分数替代人工审核，或用一次观测推断排名、收录、流量和销量。
4. 以“本地优先”为由遗漏确实需要服务端的能力；也不得以“全量迁移”为由把服务器、Redis、公开站点或多账号体系偷渡进默认桌面运行路径。
5. 尚未完成第六节所列架构决策就宣称远端分发、公开表单、多人协作等能力已等价迁移。

### 1.3 不可变约束

1. **商品事实主轴**：`Brand → Product → Fact` 是对外商品事实的主链。知识文档、标题库、主题、AI 输出和平台模板只能提供候选内容或补充证据，不能绕过 `Fact` 的来源、公开范围、有效期和审核门禁。
2. **本地优先**：安装、启动、查看、导入、切片、审核和本地导出均不依赖网络或云端账号。业务数据默认只写本机工作区。
3. **人工审核发布**：AI 质检、风险扫描和平台模板只能辅助。每次对外交付都绑定已人工审核的稿件版本及证据快照；M-GEO 默认只导出和登记人工发布结果。
4. **不承诺效果**：不得承诺或暗示 AI 首页推荐、排名、收录、引用、流量、转化或销量提升。爬虫访问、API 回答和消费者页面回答必须分开解释。
5. **密钥边界**：模型密钥保存在操作系统凭据库；业务 SQLite、日志、导出包、备份和测试 fixture 不保存明文密钥。平台账号在第一期只保存人工填写的显示标签，不保存密码、Cookie、Token 或 OAuth 凭据。
6. **跨品牌隔离**：所有业务读取和写入由原生层校验 `brandId`，不能只靠前端当前品牌过滤。

## 2. 九类能力的真实上游来源与 M-GEO 落点

以下路径均相对于上游仓库根目录；M-GEO 路径均相对于 `apps/m-geo`，除非另有说明。

### 2.1 第一类：商品与通用内容资产

**上游真实来源**

- `app/Services/GeoFlow/MaterialLibraryService.php`：素材库及库内条目的统一业务服务。
- `app/Http/Controllers/Admin/KeywordLibraryController.php`、`TitleLibraryController.php`、`ImageLibraryController.php`：关键词、标题、图片资产入口。
- `app/Services/GeoFlow/TitleGenerationCoordinator.php`、`TitleAiGenerationService.php`：标题批次及 AI 标题生成。
- `app/Services/GeoFlow/ManagedImageFileService.php`：受管图片文件、安全路径和引用清理。
- `app/Models/KeywordLibrary.php`、`TitleLibrary.php`、`ImageLibrary.php`、`Author.php`、`Category.php`：资产实体。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/core/commerce.rs`、`catalog.rs`、`batch.rs`、`feed.rs`、`feed_ops.rs` 和 `src/pages/Products.tsx`、`FeedWorkbench.tsx`。
- 新增规划 `src-tauri/src/core/assets.rs`：关键词库、标题库、分类、作者/品牌口吻和引用计数。
- 新增规划 `src-tauri/src/core/media.rs` 与受管工作区媒体目录；扩展现有 `files.rs` 处理用户显式选择的文件，禁止接受任意服务器路径。

**数据边界**

- 商品、SKU、购买信息和公开事实仍以 `m_products` 内的审核事实为准；标题或关键词不能变成无来源商品卖点。
- 标题、关键词和作者档案必须带 `brandId`；被任务、稿件或专题引用时禁止硬删除，采用引用检查或归档。
- 图片只来自用户显式选择的本地文件；保存真实 MIME、大小、宽高、哈希和版权/来源说明。私密图片不得进入公开导出包。
- AI 标题只生成待审核候选，不回写商品事实，不自动变成已审核稿件。

### 2.2 第二类：知识库、企业知识、知识事实与 URL 导入

**上游真实来源**

- `app/Models/KnowledgeBase.php`、`KnowledgeChunk.php`：知识正文、来源元数据、审核状态、切片和向量字段。
- `app/Services/GeoFlow/KnowledgeSourceParser.php`：手工内容、Markdown/Text/DOCX 的解析与合并。
- `app/Services/GeoFlow/KnowledgeChunkSyncService.php`：结构化/语义切片、暂存、向量同步和原子替换。
- `app/Services/GeoFlow/KnowledgeRetrievalService.php`：关键词、章节、元数据、审核与时效约束下的混合召回。
- `app/Services/GeoFlow/EnterpriseKnowledgeDraftService.php` 与 `app/Http/Controllers/Admin/EnterpriseKnowledgeController.php`：企业知识项目、来源、修订、校验和发布到普通知识库。
- `app/Services/GeoFlow/KnowledgeFacts/`、`app/Http/Controllers/Admin/KnowledgeFactController.php`、`KnowledgeFactGenerationController.php`：原子事实、事实值、证据、冲突和生成批次。
- `app/Services/GeoFlow/UrlImportProcessingService.php`、`app/Jobs/ProcessUrlImportJob.php`、`app/Http/Controllers/Admin/UrlImportController.php`：URL 抓取、预览、确认和恢复。

**M-GEO 对应模块**

- 第一阶段新增 `src-tauri/src/core/knowledge.rs`、`src/pages/KnowledgeWorkbench.tsx`，只实现第四节定义的 `knowledge.list/import/review`。
- 后续在同一模块增加知识修订、只读本地召回和企业知识项目；知识事实不另建“平行事实真源”，而是生成 `Product.Fact` 候选并进入现有逐条审核。
- URL 导入属于联网增强，必须等待第六节出站安全决策，不能随第一阶段文档粘贴功能一起开启。

**数据边界**

- 文档审核通过不等于商品事实审核通过；知识切片不能直接满足 `draft.review` 的 `claims[].factId` 门禁。
- 文档可绑定多个同品牌 `productIds`，原生层逐一校验归属；跨品牌绑定整体拒绝。
- `private` 文档只供本机人员阅读，默认不得进入模型请求、公开站点包或公开证据。
- 远端 URL 只作为来源元数据保存时不得被自动访问。URL 抓取、重定向和正文解析属于独立网络能力。
- 第一阶段不保存 Embedding、向量维度、Provider 或伪造的 fallback 向量字段。

### 2.3 第三类：模型、提示词与 AI 工作台

**上游真实来源**

- `app/Http/Controllers/Admin/AiModelController.php`、`AiSourceProviderController.php`、`AiPromptController.php`、`AiPromptTestController.php`：模型、Provider 和提示词管理。
- `app/Models/AiModel.php`、`AiSourceProvider.php`、`Prompt.php`：配置和绑定实体。
- `app/Services/GeoFlow/PromptTestService.php`、`PromptTestDiagnosticService.php`：提示词测试与诊断。
- `app/Services/AiWorkspace/`：会话、系统知识召回、功能注册表、只读 Agent、审批、执行防护和运行状态。
- `app/Http/Controllers/Admin/AiWorkspaceApiController.php`、`app/Models/AiWorkspaceRun.php`、`AiWorkspaceApproval.php`：工作台 API、运行和审批记录。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/ai.rs`：OpenAI 兼容文本模型、主动 consent、OS 凭据库、无自动重试、持久化任务和配置版本失效检查。
- 新增规划 `src-tauri/src/core/prompts.rs`：版本化提示词和本地测试样本；模型调用仍经 `ai.rs`，不在核心同步事务内发网络请求。
- 新增规划 `src-tauri/src/core/ai_workspace.rs` 与 `src/pages/AiWorkspace.tsx`：先做带本地证据引用的只读帮助，再增加经用户确认的结构化动作。

**数据边界**

- 保存配置不发起请求；每次测试、生成、Embedding 或可见性 API 调用都要单独 consent，并提示所选资料会离开本机且可能计费。
- Provider 密钥只进入 OS 凭据库；SQLite 仅保存引用和配置版本。备份继续排除模型设置、密钥引用、临时 prompt 和运行中任务。
- AI 工作台不能执行任意 SQL、任意文件路径或任意 action。写操作只能映射到现有白名单，并携带参数摘要、目标版本、审批和审计记录。
- 助手回答中的“已发布、已提升、已收录”等文字不构成系统事实；系统状态只从原生读回结果生成。

### 2.4 第四类：任务编排、调度、队列与批量生产

**上游真实来源**

- `app/Services/GeoFlow/TaskLifecycleService.php`：任务 CRUD、启动前检查、启停和运行查询。
- `app/Services/GeoFlow/JobQueueService.php`、`WorkerExecutionService.php`：领取、租约、重试、心跳和执行。
- `app/Jobs/ProcessGeoFlowTaskJob.php`：基于 Laravel Queue + Redis 的异步运行记录执行。
- `app/Models/Task.php`、`TaskRun.php`、`TaskSchedule.php`、`WorkerHeartbeat.php`：配置快照、运行、计划和 Worker 状态。
- `app/Services/GeoFlow/TaskRealtimeBroadcastService.php`、`HorizonMetricsAdapter.php`：实时进度和队列监控。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/core/local_jobs.rs` 的手动继续、暂停、逐步执行、重试和重启不自动跑语义，以及 `ai.rs` 的中断标记。
- 新增规划 `src-tauri/src/core/production.rs`：任务模板、不可变配置快照、运行、步骤和本地 Worker；前端新增规划 `src/pages/Production.tsx`。
- 桌面默认不引入 Redis/Horizon/Reverb。持续后台调度、多 Worker 和无人值守重试属于服务端决策。

**数据边界**

- 每次运行冻结商品版本、已审核公开事实、知识文档版本、提示词版本、模型配置版本和资产版本；后续编辑不篡改历史运行。
- 本地任务必须由用户显式启动；应用重启后将未完成任务标为 `interrupted` 或保持 `paused`，不得自动重试可能计费的模型调用。
- 取消只阻止尚未开始的本地步骤，不宣称远端已取消或费用已退还。
- 生产任务只能创建草稿和质检建议，不能直接创建 `Publication`，不能自动通过人工审核。

### 2.5 第五类：文章、专题、风险、AI 质检与审核导出

**上游真实来源**

- `app/Services/GeoFlow/ArticleGeoFlowService.php`、`ArticleContentGenerationService.php`、`ArticleWorkflowTransitionService.php`：文章生成和生命周期。
- `app/Services/GeoFlow/ArticleRiskScanner.php`、`ArticleRiskGate.php`：确定性风险扫描及门禁。
- `app/Services/GeoFlow/ArticlePublicationQualityGate.php`：发布前组合风险与 AI 质量门禁。
- `app/Services/GeoFlow/ArticleAiQualityGate.php`、`ArticleAiQualitySegmenter.php`、`ArticleAiQualityResultValidator.php`、`ArticleAiQualityInvalidationService.php`：AI 质检、分段、校验和失效。
- `app/Services/GeoFlow/ArticleAiOptimizationCoordinator.php`：优化候选、应用、取消和回滚。
- `app/Services/GeoFlow/ArticleMarkdownExportService.php`：授权范围内的 Markdown 导出。
- `app/Services/Topics/`、`app/Models/Topic.php`、`TopicRevision.php`、`TopicBuildRun.php`：专题草稿、来源失效、构建、审核和修订。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/core/claims.rs`、`core.rs` 中 `draft.generateLocal/save/review/export`、`src/pages/Content.tsx` 和证据快照。
- 新增规划 `src-tauri/src/core/content_quality.rs`：敏感词、确定性风险、AI 质检报告和失效指纹。
- 新增规划 `src-tauri/src/core/topics.rs` 与 `src/pages/Topics.tsx`：商品专题、引用稿件、来源哈希、修订和人工审核。
- 导出继续经 `draft.export`、`files.rs` 和本地站点包，不引入上游“发布即上线”的状态含义。

**数据边界**

- 正文、主张、事实关联或商品版本变化后，原人工审核、风险结果和 AI 质检结果必须失效。
- AI 质检只产生建议；即使上游支持 `auto_approved`，M-GEO 也不按分数自动审核通过。人工放行必须记录理由，且正文变化后失效。
- 专题只能引用当前品牌、已审核且证据仍有效的稿件；发布到本地站点包前再次验证所有来源哈希。
- AI 优化先产生独立候选，人工接受后才形成新草稿修订，并重新审核；不得原地保留旧审核状态。

### 2.6 第六类：网站、主题、SEO、链接池与 URL 变更

**上游真实来源**

- `app/Http/Controllers/Admin/SiteSettingsController.php`、`app/Models/SiteSetting.php`：站点设置和首页模块。
- `app/Http/Controllers/Admin/SiteThemeReplicationController.php`、`app/Services/Admin/SiteThemeReplicationService.php`、`app/Services/Admin/SiteThemeReplication/`：主题复刻、预览、迭代、版本和包。
- `app/Services/Site/`：站点 URL、文章永久链接、站点地图、外观、URL 变更及链接池站点集成。
- `app/Services/LinkPool/`、`app/Http/Controllers/Admin/LinkPoolController.php`：链接目标、活动、健康检查和主题适配。
- `app/Models/ThemeWorkspace.php`、`ThemeRevision.php`、`UrlChangeRequest.php`：主题工作区、修订和 URL 变更记录。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/core/site.rs`、`site_audit.rs`、`site_package.rs`、`feed.rs`、`readiness.rs` 和 `src/components/SiteExport.tsx`。
- 新增规划 `src-tauri/src/core/themes.rs`、`links.rs`、`url_changes.rs`：本地主题工作区、首页模块、内部链接和重定向计划。
- 当前 `site.rs` 已明确“本地、审核门禁、无网络、无发布副作用”，后续不得削弱此边界。

**数据边界**

- 本地 HTML、`sitemap.xml`、`robots.txt`、Schema、`llms.txt` 和主题包均标记为未发布；没有预期公开 HTTPS 地址时保持预览/`noindex` 语义。
- 主题复刻先支持本地模板和人工提供的合法素材；抓取参考站点需独立网络与版权决策。预览、包生成、人工部署和公开可访问是四个不同状态。
- 链接池只保存明确审核的链接及编辑理由，不承诺权重传递、抓取或排名；健康检查不得在后台静默联网。
- URL 变更先生成计划和本地校验报告；只有人工部署后登记的证据才能标记已实施，不能改写历史公开结果。

### 2.7 第七类：分发、托管站点、人工发布与浏览器助手

**上游真实来源**

- `app/Services/GeoFlow/DistributionOrchestrator.php`、`DistributionPayloadBuilder.php`、`DistributionRetryPolicy.php`：分发编排、载荷和重试。
- `app/Services/GeoFlow/GeoFlowAgentPublisher.php`、`WordPressRestPublisher.php`、`GenericHttpApiPublisher.php`：三类真实远端发布适配器。
- `app/Services/GeoFlow/DistributionChannelDeletionService.php`、`DistributionChannelOperationLeaseService.php`：渠道删除影响预检、租约和防并发。
- `app/Services/HostedSites/`、`app/Http/Controllers/Admin/HostedSiteController.php`：托管站点分配、激活、质量预检、发布和生命周期。
- `app/Services/GeoFlow/ManualPublicationService.php`、`ManualPublicationDuplicateDetector.php`：人工发布工单、重复提示、状态迁移和完成回执。
- `app/Services/BrowserOperations/`、`browser-extension/`、`app/Http/Controllers/Api/V1/BrowserManualPublicationController.php`：设备授权、领取、心跳、回执和受控填充。

**M-GEO 对应模块**

- 复用现有 `publication.add`、`src/pages/Publications.tsx`、`publicationStatus.ts` 和固定 `contentSnapshot`。
- 新增规划 `src-tauri/src/core/manual_publications.rs`：草稿、待执行、执行中、完成、失败、跳过、取消和结果未知工单；账号只存显示标签。
- 新增规划 `src-tauri/src/core/channel_packages.rs`：WordPress/通用 HTTP/平台专用交付包和字段适配报告，默认只导出，不发送。
- 浏览器扩展、自动远端分发和真实托管必须等待第六节决策；不得用桌面定时任务模拟已具备服务端能力。

**数据边界**

- 只有当前已人工审核、事实和证据快照有效的稿件版本可进入 `ready`；编辑后旧工单必须重新核对。
- `completed` 必须有人工回填的真实公开 URL 和完成时间；导出、复制、扩展填草稿、健康检查成功均不得置为 `completed`。
- 超时或回执缺失使用 `outcome_unknown`，先人工对账，禁止自动重发。重复提示只比较同品牌载荷指纹、目标账号标签和 URL。
- 渠道交付包不包含密码、Cookie、Token、私密事实、内部备注或本机路径。
- 若未来批准服务端分发，必须保留人工批准、幂等键、操作收据、远端 ID、重试预算和删除影响清单；本地删除不宣称远端内容已删除。

### 2.8 第八类：内容/流量/分发/线索分析与 AI 可见性

**上游真实来源**

- `app/Services/Admin/Analytics/`：总览、访问日志、分发、线索和 AI 可见性分析。
- `app/Http/Controllers/Admin/ContentAnalyticsController.php`、`TrafficAnalyticsController.php`、`DistributionAnalyticsController.php`、`LeadAnalyticsController.php`、`AiVisibilityAnalyticsController.php`：各类分析页面。
- `app/Services/GeoFlow/AiVisibility/`、`app/Models/AiVisibilityRun.php`、`AiVisibilitySource.php`：Provider 调用、回答、来源和竞品检测运行。
- `app/Http/Controllers/Admin/LeadController.php`、`LeadFormController.php`、`app/Models/LeadForm.php`、`LeadSubmission.php`：公开表单和线索管理。

**M-GEO 对应模块**

- 复用现有 `src-tauri/src/core/observations.rs`、`experiments.rs`、`experiment_evidence.rs`、`recommendation_transitions.rs`、`attribution*.rs`、`optimization*.rs` 和 `src/pages/Visibility.tsx`、`AttributionWorkbench.tsx`。
- 新增规划 `src-tauri/src/core/traffic.rs`、`leads.rs`：人工导入访问日志和线索，保留来源、口径、覆盖区间和缺测状态。
- 可见性 API 增强复用 `ai.rs` 的主动联网边界，新增规划 `src-tauri/src/core/visibility_runs.rs` 保存原始回答和来源。

**数据边界**

- 原始观察提交记录不可变，人工核对注解按版本追加；延续现有 `observations.rs` 语义。`recorded`、`simulated`、`unspecified` 不得混算。
- 消费者页面与 API、`baseline` 与 `followup`、模型标签、搜索模式、测试时间和问题快照必须保留。API 结果不能冒充消费者页面结果。
- 缺测、失败、未核实、无引用和“未提及”是不同状态；单次或多次样本均不得外推全平台排名。
- 爬虫日志只表示抓取活动，不等于收录或引用。归因数据只按导入口径呈现，不自动建立因果。
- 线索包含个人信息，列表最小化展示，导出需显式操作并记录审计；桌面版只接收人工导入，公开采集端点需要服务端。

### 2.9 第九类：API/CLI、权限、安全、审计、备份、更新与运维

**上游真实来源**

- `routes/api.php`、`routes/web.php`：API v1、后台、浏览器和管理操作真实路由面。
- `bin/geoflow`、`packages/geoflow-cli/`：CLI、安装和发布工具。
- `app/Http/Controllers/Admin/AdminUserController.php`、`ApiTokenController.php`、`SecuritySettingsController.php`、`AdminActivityLogController.php`：管理员、Token、安全设置和审计。
- `app/Services/Outbound/SafeOutboundHttpClient.php`：协议、DNS、私网、重定向和响应大小防护。
- `app/Services/Deployment/`、`app/Services/SystemUpdater/`、`app/Http/Controllers/Admin/SystemUpdateController.php`、`CoordinatedUpdaterController.php`：升级计划、日志、备份、回滚和协调更新。
- `app/Console/`、`app/Jobs/`：维护命令、恢复任务和调度工作。
- `app/Services/GeoFlow/AnonymousUsageTelemetry.php`：上游匿名使用统计；它不是 M-GEO 默认必须开启的能力。

**M-GEO 对应模块**

- 复用现有 `src/api.ts`、`src-tauri/src/runtime.rs`、`core.rs`、`backup.rs`、`files.rs`、`desktop.rs` 的白名单、事务、加密备份和原子恢复。
- 新增规划 `apps/m-geo-cli/`：直接链接同一 Rust 核心库的本地 CLI，禁止绕过原生校验直接写 SQLite。
- 新增规划 `src-tauri/src/core/audit.rs` 或按现有 `m_audit` 扩展统一审计查询；敏感正文和密钥不进入日志。
- 桌面更新采用签名包和 Tauri 更新机制，不能照搬 Laravel 的 Artisan/Composer/队列升级步骤。公开 REST、多用户和服务端更新需独立服务架构。

**数据边界**

- 所有新 action 必须加入 `CORE_ACTIONS` 或明确的控制器白名单；内部 prepare/commit 不对前端开放。
- 单机多品牌不是多人权限系统，不能伪造管理员角色、审批人或 Token scope。需要真实身份时必须引入认证服务。
- 备份覆盖新增业务表，但继续排除模型设置、密钥、临时 AI 任务和瞬态 prompt；恢复前验证 schema、外键、领域状态和备份结构。
- 默认不迁移开启匿名遥测。任何未来遥测都必须单独、明确、可撤回地 opt-in，且不得包含商品、稿件、URL、线索或密钥。
- 桌面和服务端不得同时直接写同一个 SQLite 文件；若引入服务端同步，必须先定义独立数据库、所有权、冲突和离线同步协议。

## 3. 统一领域与工程约束

### 3.1 原生写入约束

1. 新对象使用 UUID 字符串 ID、camelCase JSON、ISO 8601 UTC 时间；日期有效期沿用 `YYYY-MM-DD` 和 UTC 末日语义。
2. 写操作使用 `expectedRevision` 乐观锁和 `TransactionBehavior::Immediate`；跨表状态变化在同一事务提交。
3. 业务表都有 `brand_id`。跨表引用采用同品牌复合外键，读取时使用 `id + brand_id`，不能只检查对象存在。
4. 错误返回安全中文消息，不向 UI 暴露原始 SQL、Provider 响应、密钥或本机私有路径。
5. 关键写入追加 `m_audit`。历史运行、审核、发布和观察保留快照；派生缓存必须可重建且不能反当事实源。
6. 外部输入先规范化再哈希和校验；拒绝重复 JSON key、NUL、超限内容和非法枚举。客户端传入的审核时间、证据快照和远端状态不可信。
7. 大文档、媒体和运行记录不塞进 `workspace.snapshot`。该快照已有数量边界，新增页面使用独立分页/详情 action，避免把全工作区变成超大 JSON。

### 3.2 数据库与备份升级门禁

第一期知识表将数据库从 5 升至 6，实施时必须同一变更完成：

- `core.rs` 初始化、`m_core_meta` 和领域 `validate_database`。
- `runtime.rs` 可接受版本、旧版本升级路径、升级前快照和拒绝未来版本。
- `backup.rs` 的版本白名单、参考 schema 比对、恢复后版本、备份检查和计数。
- 旧版本 1—5 的升级 fixture、损坏 schema、外键错误、跨品牌引用和恢复中断测试。
- 加密备份往返后文档、切片、审核历史逐字段一致，且模型配置与密钥仍被排除。

不得只把两处数字从 5 改成 6；当前严格 schema 比较会使遗漏表或索引的备份直接不可用。

## 4. 知识库第一阶段实际契约

### 4.1 已实现范围

实现品牌内文档列表/详情、人工粘贴 Markdown/Text、确定性本地切片、文档人工审核和审核历史。所有测试使用虚构资料；不调用真实模型或平台。

明确不实现：文件上传、DOCX、URL 抓取、企业知识项目、Markdown 标题层级解析、AI 语义切片、Embedding、向量字段、远程检索、自动事实抽取、自动商品事实写入、自动草稿生成和任何后台同步。`knowledge.*` 走同步 `core::handle`，不创建 HTTP client 或 Worker。

### 4.2 Action 与数据

- `knowledge.list {brandId}` 返回摘要数组，按 `createdAt DESC,id DESC` 排列；不含正文、切片和审核历史。`{brandId,documentId}` 返回单个详情，增加 `content/chunks/reviews`。
- 摘要字段：`id/brandId/productId/title/source/format/effectiveAt/risk/reviewStatus/reviewedAt/revision/chunkCount/createdAt/updatedAt`。`productId` 可为空，关联商品必须属于同品牌。
- 切片字段：`{id,ordinal,text,sha256}`；审核记录字段：`{id,documentId,revision,decision,note,createdAt}`。
- `knowledge.import` 输入：`{brandId,productId,title,source,format,effectiveAt,risk,content,requestId}`。`format` 为 `markdown|text`，`risk` 为 `low|medium|high`。标题最多 300 字、来源最多 2,000 字、正文最多 200,000 个 Unicode 字符；拒绝空正文、NUL、非法日期和未知字段。
- 每次新请求创建 `revision=1`、`reviewStatus=pending` 的文档；同品牌同 `requestId` 同载荷重放返回原文档，不同载荷冲突。无幂等键时不提供静默覆盖或重复写入语义。
- `knowledge.revise` 在导入字段上增加 `id/expectedRevision`；保留文档 ID、创建时间和全部审核历史，更新正文与元数据并重新切片，revision 加一，状态回到 `pending`，旧审核时间清空。修订与导入共用品牌内请求幂等约束，陈旧 revision 和跨品牌操作拒绝。
- `knowledge.review {brandId,id,expectedRevision,decision,note}` 接受 `approved|rejected`；驳回理由必填，理由最多 2,000 字。有效审核使 revision 加一并追加历史，陈旧 revision 拒绝。
- `knowledge.recall {brandId,query,productId?,limit?}` 是只读本地关键词召回：query 去空白后必填且最多 200 字，limit 默认及最大均为 20；仅返回当前品牌当前已审核文档，传商品时仅含通用和该商品文档。中文按规范化子串匹配，ASCII 英文／数字按空白分词后的完整词匹配；全部去重关键词须命中标题或切片。结果按分数降序及文档 ID、切片序号、切片 ID 确定性排序，仅含来源、审核版本和命中切片等最小追溯字段，不含完整正文、审核历史、请求哈希或模型配置，不写审计、稿件或知识文档。该能力不是 Embedding、语义检索或外部平台效果测试。
- 当前 UI 只允许审核待审核文档；已审核或已驳回文档通过“修订此文档”形成新修订后重新审核，不支持在同一修订上反复审核。
- 知识审核不修改商品确认、Fact 审核、稿件审核或发布状态，也不能满足 `draft.review` 的 Fact 门禁。
- 每品牌最多 100 份文档。其他品牌或不存在的 ID 统一返回“知识文档不存在或不属于当前品牌”。

### 4.3 确定性本地切片

正文先统一 LF，以空行分段，按原顺序聚合到最多 1,200 个 Unicode 字符；超长段落按字符边界切分，重叠为 0。每片保存从 0 开始的 ordinal、原文和 SHA-256；切片 ID 由文档 ID、序号、文本哈希确定性生成。单文档最多 200 片，超限整体拒绝。此版本不按 Markdown 标题建章节路径，也不生成文档内容哈希或切片 manifest 字段。

### 4.4 表、审计、备份与验收

新增 `m_knowledge_documents` 与 `m_knowledge_reviews`。文档 JSON 保存正文及切片，审核历史独立保存；表包含品牌、商品复合外键、请求幂等唯一约束和索引。导入、修订及审核经现有核心事务写入审计，幂等重放不重复写审计；正文和切片不进入 `workspace.snapshot`。

第一阶段验收以实际测试为准，覆盖品牌隔离、跨品牌商品拒绝、幂等重放及冲突、输入边界、Unicode/CRLF/长文切片、审核历史与 revision 冲突、知识审核与 Fact/稿件门禁分离，以及 schema 5 → 6 升级和加密备份恢复。完整前端、Rust 测试和构建结果只在实际运行后记录；未运行或真实桌面未操作的检查不标为通过。

## 5. 分阶段实施、依赖与验收

### P0：基线冻结与迁移追踪

**依赖**：无。

**工作**：固定上游 commit；建立九类能力到上游源码、M-GEO action、数据表、UI、测试和许可说明的追踪表。为第六节每项决策建立 ADR。根 README 与应用 README 的版本口径不同，实施记录必须注明实际源码和构建，不把源码候选当已发布安装包。

**验收**：九类没有“未分配”项；每项区分现有、复用、新增、桌面等价、服务端待决策。许可和版权文件清单齐全。所有 fixture 使用虚构品牌、商品、账号标签和无效密钥。

### P1：知识库最小闭环

**依赖**：P0；现有品牌、原生 dispatch、事务、备份体系。

**工作**：只交付第四节契约及知识 UI，不夹带 Embedding、URL 导入或 AI 生成。

**验收**：按第四节 4.4 的实际实现范围运行测试；新增 action 白名单、审计、备份、恢复和跨品牌测试齐全。

### P2：通用资产与进阶知识

**依赖**：P1；媒体 ADR；URL 导入若纳入则需 NET ADR 先批准。

**工作**：关键词/标题/分类/作者、受管图片、知识修订和本地关键词召回；企业知识项目从人工来源生成待审草稿；知识事实只生成商品 Fact 候选。DOCX 可复用现有显式文件选择和安全抽取，URL 导入单独交付，不能捆绑默认开启。

**本次已交付子集**：知识文档修订及旧审核失效；已审核知识的只读本地关键词召回；关键词/标题人工创建、修订、审核和全历史；品牌隔离、乐观锁、创建/知识修订幂等；schema 6 → 7、审计及备份恢复。对应 UI 为 `AssetLibrary.tsx`、知识修订表单与编辑器 `KnowledgeRecall.tsx`。召回只返回当前品牌、当前审核版本及当前商品范围内的命中切片，提供来源、文档审核版本、切片序号和 SHA-256；不自动插入正文、生成 Fact、审核稿件或联网。内容编辑器还可实际使用已审核标题/关键词并保存资产 ID；原生审核重新检查资产状态、商品归属和标题/正文使用情况，冻结资产文本、来源、审核版本与时间。导出附带快照并对照资产历史校验；资产后续修订不改写历史稿件。资产采用可恢复归档而非物理删除，归档后禁止修订、审核和新稿使用，历史快照仍可导出；恢复不等于重新审核，快照仍绑定最近一次实际审核版本。无 AI 生成、联网或发布。

**尚未完成**：分类/作者、受管图片、Embedding／语义召回、企业知识项目、候选 Fact、DOCX/URL 导入。不得把本子集称为 P2 全部完成。

**验收**：本次子集须通过知识修订失效、召回品牌／商品／审核隔离、输入与匹配边界、确定性排序、最小字段、只读性、界面迟到响应与重试、100 文档 × 200 切片上限、重开及备份恢复，以及资产历史与归属、schema 6 升级、前端流程、未审核/未使用/跨商品资产拦截、伪造快照拒绝、资产后续修订与历史导出、归档恢复顺序和归档后历史导出测试。完整 P2 仍须覆盖媒体 MIME/路径/大小、公私可见性和候选 Fact 人工审核。无真实上传、无外部 URL 请求；未批准 NET ADR 时 URL 导入明确保持不可用。

### P3：生产任务、文章质量与 AI 配置

**依赖**：P1、P2；现有 `ai.rs` 和 `local_jobs.rs`。

**工作**：版本化提示词、任务快照、显式启动的本地运行、暂停/取消/重试、文章生成、确定性风险、AI 质检候选和失效。先小批量，再批量；无自动发布。

**验收**：同一运行快照可重放；重启不自动跑；模型配置/商品/知识变化使提交失效；取消与失败保留已完成草稿但不伪造成功；AI 高分仍须 `draft.review`；私密事实和私密知识不进入模型请求。

### P4：专题、网站、主题、SEO 与链接

**依赖**：P3 的已审核稿件；现有本地站点包。

**工作**：专题修订、来源失效、首页模块、本地主题工作区、链接池、URL 变更计划、站点包及审计。主题远程复刻和远程发布分别等待 NET/REMOTE ADR。

**验收**：未审核或过期引用阻断打包；HTML 转义和 CSP 不回归；无公开地址时 `noindex`；manifest 明确 `not-published`；主题预览、包生成、人工部署和公开可访问状态不混用；链接与重定向不产生排名承诺。

### P5：人工发布工单与渠道交付包

**依赖**：P3、P4；现有 `publication.add`。

**工作**：人工工单、账号显示标签、分配 placeholder（单机仅本地执行人标签）、重复提示、交付包、复制/导出、结果回填和版本台账。浏览器扩展仅在 EXT ADR 批准后实施。

**验收**：未审核稿件不能进入 ready；完成必须有真实 URL 字段的人工回填，但自动化测试只使用保留的虚构示例 URL且不访问；导出和填充不等于完成；超时进入结果未知；包内无密钥、私密事实和本机路径；发布快照不可变。

### P6：观察、分析、线索与只读 AI 工作台

**依赖**：P1 的本地知识证据、P3 的模型边界、P5 的发布记录。

**工作**：人工流量/线索导入、AI 可见性运行记录、分析报告和带引用的只读帮助。公开表单与自动消费者观测不在桌面默认路径。

**验收**：原始证据不可变、注解版本化；模拟与实录、API 与消费者、缺测与未提及分开；报表能追溯到原始记录和统计口径；AI 工作台不伪造 action 结果；线索导出显式且有审计。

### P7：集成、运维及经批准的服务端能力

**依赖**：P0—P6；所有相关 ADR 已批准并有独立安全评审。

**工作**：本地 CLI、签名桌面更新、审计和维护工具；若批准服务端，再实施 REST/API Token、多人 RBAC、调度、公开表单、托管、远程分发、浏览器设备授权及协调更新。每项有独立里程碑，不能以一个“服务端已完成”概括。

**验收**：本地 CLI 与 UI 使用同一领域校验；更新前备份和恢复演练通过；服务端有独立数据库和真实身份，桌面不共享写 SQLite；远端操作具备人工批准、幂等、收据、对账和撤销；公开端点完成权限、限流、SSRF、隐私和恢复测试。未实施的决策项继续阻塞“全量迁移完成”声明。

### 每阶段共同验证命令

在 `apps/m-geo` 使用锁文件和仓库规定工具链执行：

```sh
pnpm test
pnpm typecheck
pnpm build
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --features desktop --all-targets -- -D warnings
```

阶段验收还应包含对应 UI 流程和失败路径。本地测试通过不等于远程 CI、真实平台、公开部署或消费者效果通过。

## 6. 必须先做的桌面/服务端架构决策

| 决策 | 当前安全默认 | 需要决定的内容 | 对全量迁移的影响 |
|---|---|---|---|
| NET：URL 导入、主题参考抓取、链接健康检查 | 全部关闭；只允许手工粘贴和本地文件 | DNS 固定、私网/回环/危险端口阻断、重定向、响应大小、MIME、超时、版权与 consent | 未批准前相应上游能力不能报完成 |
| EMB：Embedding 与语义切片 | 仅确定性切片，无向量 | 本地模型还是远端模型、版本指纹、费用、重建、旧 ready 切片保留 | 不能以哈希假向量冒充语义检索 |
| REMOTE：WordPress/通用 HTTP/Agent 分发 | 只生成交付包，人工发布 | 是否改变“不自动发布”产品边界；服务端、人工批准、密钥、幂等、结果未知、删除对账 | 若坚持原样自动分发，需先变更产品要求；当前约束下只能迁移人工交付等价路径 |
| HOST：托管站点和定时上线 | 本地静态站点包，不托管 | 独立服务端、域名、证书、构建、公开缓存、配额、生命周期和回滚 | 桌面本地包不等于托管能力完成 |
| FORM：公开线索表单和访问采集 | 人工导入 | 公开服务端、认证、限流、隐私、留存、删除和导出 | 无服务端则只有导入/管理，不宣称公开采集已迁移 |
| RBAC：多人、分配、审批、API Token | 单本机用户，无伪造角色 | 身份提供方、角色、会话撤销、审计 actor、最小 scope | 多人能力需要服务端，品牌隔离不能替代权限 |
| EXT：浏览器运营助手 | 复制/导出，人工点击平台最终提交 | 扩展权限、设备授权、loopback/native messaging 或服务端、撤销、页面适配和人工最终确认 | 不得保存平台凭据、绕过验证或自动最终发布 |
| SCHED：常驻调度和多 Worker | 应用内显式启动，重启不自动跑 | 是否引入服务、租约、心跳、恢复和费用控制 | 不用桌面开机自启偷代无人值守服务 |
| API/CLI：外部集成 | 本地 CLI 复用 Rust 核心 | 本地 CLI 的离线契约；REST 的服务端数据库、认证和同步 | 不允许多进程直接共写 SQLite |
| SYNC：桌面与服务端数据关系 | 各自独立，不自动同步 | 主记录所有权、离线冲突、品牌迁移、密钥排除和删除传播 | 未定协议前不得开放双端写 |
| UPDATE：桌面与服务端更新 | 桌面签名更新和加密备份 | Tauri 更新渠道、签名、回滚；服务端独立协调更新 | 不照搬 Artisan/Composer 到桌面 |
| OBSERVE：AI 可见性 API | 人工观察优先 | Provider、费用、原始回答保存、API/消费者标签和采样口径 | 只能报告观测，不承诺排名或消费者表现 |

ADR 必须记录选择、 rejected alternatives、数据流、密钥位置、威胁模型、人工审批点、失败/恢复、测试和回滚。选择“暂不做”可以阻塞对应里程碑，但不能把全量范围改写为已完成。

## 7. 禁止事项与测试数据规则

1. 禁止在开发、测试、截图、文档、fixture、日志和导出中使用真实平台账号、真实 Cookie/Token/API Key、真实客户资料或真实线索。
2. 禁止以测试为由发起真实发布、真实删除、真实付款、真实模型计费或真实平台提交。网络层使用显式 fake transport；不访问示例 URL。
3. 禁止伪造“已发布、已收录、排名第一、推荐提升、流量增长、销量增长”等平台效果。状态测试只验证状态机和证据格式，不生成宣传性结论。
4. 模拟数据必须带 `evidenceMode=simulated` 或等价标识，不进入真实业务汇总；本地自动化通过不能写成平台验收通过。
5. 禁止自动登录第三方平台、绕过验证码/风控、保存平台密码或自动点击最终发布。
6. 禁止把私密商品事实、内部备注、本机绝对路径、完整 prompt、密钥或未脱敏 Provider 响应写入公开包和审计日志。
7. 禁止为了让验收变绿而降低跨品牌、审核、备份原子性、密钥不落库和人工发布门禁。

## 8. 关键架构风险与控制

| 风险 | 具体后果 | 必须控制 |
|---|---|---|
| 上游能力地图滞后于 commit | 漏掉专题、知识事实、链接池、URL 变更等真实能力 | 以源码和路由为基线维护追踪表；每次上游升级重新盘点，不只读旧指南 |
| “全量”与“不自动发布”冲突 | 为凑功能偷偷加入远端自动发布，或把导出谎报为分发完成 | REMOTE/HOST ADR 明示产品边界；全量声明只覆盖已实现的人工等价或批准的服务端能力 |
| 知识证据洗白商品事实 | 未审核文档经 AI 变成“有依据”的公开主张 | 知识审核与 Fact 审核分离；所有公开主张最终绑定已审核 Fact 和冻结快照 |
| 数据库版本与备份严格校验不同步 | 新表后旧备份、升级或恢复直接失败 | schema、初始化、领域校验、runtime、backup 同一变更交付，覆盖 1—5 升级和往返 |
| 大对象污染 workspace.snapshot | 知识正文/切片使 UI 超限、品牌切换变慢、截断误解 | 独立 list/detail 和分页；快照不装入大文档和媒体 |
| 本地任务与远端副作用语义混淆 | 重试造成重复计费或重复发布，取消被误报为远端取消 | 本地与远端步骤分型；模型不自动重试；远端使用收据和 outcome_unknown，先对账 |
| 桌面与服务端共写 SQLite | 锁冲突、损坏、跨品牌或审核门禁被绕过 | 服务端独立数据库；同步前定义所有权和冲突协议，所有写经同一领域服务 |
| AI 质检替代人工 | 自动通过错误、过期或虚构商品内容 | AI 仅建议；人工审核、变更失效和放行理由由原生层强制 |
| 观测数据被包装成排名效果 | 虚构平台表现和经营结论 | 原始证据不可变，区分 API/消费者和缺测；报告只陈述样本，不做无依据外推 |
| 密钥与隐私面扩大 | 分发、扩展、线索和日志泄露敏感信息 | OS 凭据库、账号标签、最小化日志、显式导出、公开包扫描和权限审计 |
| AGPL 来源与第三方声明遗漏 | 分发或网络服务许可不合规 | 保留上游 NOTICE/LICENSE，移植清单对应源码路径；新增服务端时评估网络交互源码提供义务 |
| 把预览版能力当生产事实 | 上游 beta 的未完成远端主题/更新能力被写成 M-GEO 已支持 | 验收以 M-GEO 自身测试和真实读回为准，分别记录未验证平台和架构限制 |

## 9. 交付完成检查

每次能力合并前必须能回答：

1. 上游来源文件、行为、异常和状态机是否已核对到固定 commit？
2. M-GEO 的 action、数据表、品牌外键、revision、审核和审计在哪里？
3. 是否仍由商品 Fact 决定公开事实，由人工决定审核和发布？
4. 是否有网络、密钥、公开端点、常驻 Worker 或第三方账号副作用？对应 ADR 是否先批准？
5. 失败、取消、超时、重启、重复请求和恢复是否会留下真实且可解释的状态？
6. 是否使用虚构数据且没有真实账号、密钥、发布和平台效果断言？
7. 前端、Rust、备份恢复、跨品牌和许可检查是否全部通过？

只有九类追踪项均满足上述问题、相关 ADR 已落地为已验收实现，才能声明“GEOFlow 全量能力迁移完成”。
