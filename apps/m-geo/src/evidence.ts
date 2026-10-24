import type {Campaign,EvidenceMode,Observation} from './types';
export const evidenceLabels:Record<EvidenceMode,string>={recorded:'人工实测记录',simulated:'模拟样本（非真实平台结果）',unspecified:'历史性质未分类'};
export function campaignMode(campaign?:Pick<Campaign,'evidenceMode'>):EvidenceMode {
  return campaign?.evidenceMode==='recorded'||campaign?.evidenceMode==='simulated' ? campaign.evidenceMode : 'unspecified';
}
export function observationMode(observation:Observation,campaigns:Campaign[]=[]):EvidenceMode {
  return campaignMode(campaigns.find(c=>c.id===observation.assessment?.campaignId));
}
export function evidenceCounts(observations:Observation[],campaigns:Campaign[]=[]){
  const counts={recorded:0,simulated:0,unspecified:0};
  for(const observation of observations) counts[observationMode(observation,campaigns)]++;
  return counts;
}
