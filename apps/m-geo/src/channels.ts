import type {Kind} from './types';

export type ChannelId='jd'|'tmall'|'douyin'|'wechat_channels'|'xiaohongshu'|'kuaishou'|'vipshop';
export interface ChannelProfile{
 id:ChannelId;
 label:string;
 template:string;
 kind:Kind;
 description:string;
 reminder:string;
 ruleLabel:string;
 ruleUrl:string;
 checkedAt:string;
}
export const channelProfiles:Record<ChannelId,ChannelProfile>={
 jd:{id:'jd',label:'京东',template:'商品参数与详情',kind:'description',description:'完整参数、选购信息和使用说明。',reminder:'按当前类目核对必填参数；价格、库存和促销以实际销售页面为准。',ruleLabel:'京东商品详情发布规范',ruleUrl:'https://helpcenter.jd.com/vender/issue/1000-47828.html',checkedAt:'2026-10-24'},
 tmall:{id:'tmall',label:'天猫',template:'类目资料与规格选择',kind:'description',description:'标题事实、SKU 属性、详情和规格选择指南。',reminder:'按当前类目 Schema 核对字段，确保标题、属性、图片和 SKU 一致。',ruleLabel:'淘宝／天猫商品发布说明',ruleUrl:'https://open.taobao.com/doc.htm?docId=119447&docType=1',checkedAt:'2026-10-24'},
 douyin:{id:'douyin',label:'抖音',template:'短视频与商品图文',kind:'description',description:'口语化短视频脚本和商品展示结构。',reminder:'发布前核对账号权限、商品关联和当前图文／视频规范。',ruleLabel:'抖音商品短视频说明',ruleUrl:'https://school.jinritemai.com/doudian/wap/article/101823',checkedAt:'2026-10-24'},
 wechat_channels:{id:'wechat_channels',label:'视频号／微信小店',template:'口播与公众号说明',kind:'guide',description:'视频号口播、直播问答和公众号延伸阅读。',reminder:'分别核对微信小店商品信息、视频号内容和公众号发布要求。',ruleLabel:'微信小店官方文档',ruleUrl:'https://developers.weixin.qq.com/doc/store/shop/',checkedAt:'2026-10-24'},
 xiaohongshu:{id:'xiaohongshu',label:'小红书',template:'场景选购与商品笔记',kind:'guide',description:'真实场景、选购问题和商品笔记草稿。',reminder:'不得伪造个人体验、评价或亲测经历；商业内容按平台要求标识。',ruleLabel:'小红书专业号介绍',ruleUrl:'https://fe-video-qc.xhscdn.com/fe-platform/68e374e4257bd30bc07eed15c06cb8b9a51f971b.pdf',checkedAt:'2026-10-24'},
 kuaishou:{id:'kuaishou',label:'快手',template:'短视频与直播问答',kind:'description',description:'口语化演示脚本、规格解释和直播问答。',reminder:'按当前商家后台核对商品发布、带货权限和内容规范。',ruleLabel:'快手电商学院资料',ruleUrl:'https://static.yximgs.com/udata/pkg/KS-Merchant-Ops/kuaishoudianshangbaodian.pdf',checkedAt:'2026-10-24'},
 vipshop:{id:'vipshop',label:'唯品会',template:'供应商商品资料',kind:'description',description:'SPU／SKU、规格尺码和简洁商品资料。',reminder:'按供应商后台及类目要求核对尺码、规格和资料字段。',ruleLabel:'唯品会商品资料接口说明',ruleUrl:'https://vop.vip.com/doccenter/viewdoc/133',checkedAt:'2026-10-24'},
};
export const channelList=Object.values(channelProfiles);
export function channelLabel(id:ChannelId|null|undefined){return id?channelProfiles[id].label:'通用内容';}
