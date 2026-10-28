import type {ChannelId} from './channels';

export const PUBLIC_SOURCE_CHANNEL_PACK_VERSION='public-source-manual-1.0.0';
export type PublicSourceChannelId='brand_website'|'help_center'|'trusted_media_contribution'|'industry_community'|'documentation_center'|'public_qa_manual';
export type PublicationChannelCategory='ecommerce'|'public_source';
export interface PublicSourceReadBack{
 supported:boolean;
 mode:'manual_url'|'manual_url_or_screenshot';
 instructions:string;
}
export interface PublicSourceChannelProfile{
 id:PublicSourceChannelId;
 label:string;
 category:'public_source';
 purpose:string;
 requiredEvidence:string[];
 publishMode:'manual_only';
 autoPublish:false;
 readBackSupport:PublicSourceReadBack;
 complianceReminder:string;
}

export const publicSourceChannels:Record<PublicSourceChannelId,PublicSourceChannelProfile>={
 brand_website:{
  id:'brand_website',label:'品牌官网',category:'public_source',
  purpose:'沉淀品牌可直接维护的商品介绍、选购指南与适用条件，作为公开事实的原始出处。',
  requiredEvidence:['已审核公开事实及其具体来源','页面负责人与拟发布路径','发布后的公开 URL 与人工回读记录'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url',instructions:'由负责人发布后，在无登录或读者视图人工打开 URL，核对正文、更新时间与可见范围。'},
  complianceReminder:'不得把草稿或后台预览记作已发布；旧页面更新、下线与事实有效期须由负责人同步处理。'
 },
 help_center:{
  id:'help_center',label:'品牌帮助中心',category:'public_source',
  purpose:'回答使用、限制、售后和故障排查问题，减少指南与正式服务政策之间的歧义。',
  requiredEvidence:['已审核的使用或售后事实','当前服务政策与适用范围','文章负责人及公开帮助页 URL'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url',instructions:'人工检查公开访问、问题答案、政策链接与更新时间，并记录回读日期。'},
  complianceReminder:'不得扩大保修、退换或服务承诺；涉及个案时引导至正式客服流程，不收集或公开个人信息。'
 },
 trusted_media_contribution:{
  id:'trusted_media_contribution',label:'可信媒体投稿',category:'public_source',
  purpose:'向具有编辑审核流程的行业或大众媒体提交事实清楚的科普、选购或技术稿件。',
  requiredEvidence:['可定位的事实来源与必要授权','商业关系及利益冲突说明','媒体接收要求、编辑确认与最终公开 URL'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url',instructions:'仅在编辑实际刊发后人工回读最终版本；投稿、送审或录用不等于发布。'},
  complianceReminder:'遵守媒体编辑与商业标识要求，不购买或伪造媒体报道，不把投稿成功表述为传播效果保证。'
 },
 industry_community:{
  id:'industry_community',label:'行业社区',category:'public_source',
  purpose:'在允许机构发言的专业社区参与技术讨论、答疑和经验整理，回应真实问题。',
  requiredEvidence:['与讨论直接相关的公开事实来源','账号身份与商业关系说明','社区规则核对及人工发布记录'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url_or_screenshot',instructions:'人工回读公开帖子；若社区限制公开访问，保留合规截图并明确可见范围，不伪称全网公开。'},
  complianceReminder:'禁止灌水、刷量、伪造用户身份或亲身体验；按社区规则披露品牌关系并尊重删改与归档机制。'
 },
 documentation_center:{
  id:'documentation_center',label:'公开文档中心',category:'public_source',
  purpose:'维护结构化规格、操作步骤、版本说明与限制条件，便于读者定位稳定资料。',
  requiredEvidence:['已审核规格或操作事实','适用版本、条件和限制','文档 owner、版本号及公开文档 URL'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url',instructions:'人工核对公开页面的版本、目录、正文和附件链接，记录文档版本与回读日期。'},
  complianceReminder:'不得混入私有事实、内部备注、密钥或个人信息；版本变更时同步更新适用范围和历史说明。'
 },
 public_qa_manual:{
  id:'public_qa_manual',label:'公开问答（人工）',category:'public_source',
  purpose:'由真实授权人员针对公开问题逐条作答，提供直接答案、条件与可核验来源。',
  requiredEvidence:['问题原文与公开页面上下文','回答所用已审核公开事实及具体来源','答主身份、商业关系和人工发布凭证'],
  publishMode:'manual_only',autoPublish:false,
  readBackSupport:{supported:true,mode:'manual_url_or_screenshot',instructions:'人工打开回答页面核对完整答案与来源；无法公开访问时记录实际可见范围，不作公开传播假设。'},
  complianceReminder:'禁止机器批量作答、自问自答、虚假身份和未披露推广；只回答已人工确认且适合公开的问题。'
 }
};
export const publicSourceChannelList=Object.values(publicSourceChannels);
export function isPublicSourceChannel(id:string|null|undefined):id is PublicSourceChannelId{
 return Boolean(id&&Object.hasOwn(publicSourceChannels,id));
}
export function publicSourceChannelLabel(id:PublicSourceChannelId|null|undefined):string{
 return id?publicSourceChannels[id].label:'公开信源';
}
/** Existing ChannelId values are commerce surfaces; this pack never reclassifies them as open sources. */
export function publicationChannelCategory(id:ChannelId|PublicSourceChannelId):PublicationChannelCategory{
 return isPublicSourceChannel(id)?'public_source':'ecommerce';
}
export function getPublicSourceChannel(id:string|null|undefined):PublicSourceChannelProfile|null{
 return isPublicSourceChannel(id)?publicSourceChannels[id]:null;
}
