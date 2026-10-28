import {describe,expect,it} from 'vitest';
import {PUBLIC_SOURCE_CHANNEL_PACK_VERSION,getPublicSourceChannel,isPublicSourceChannel,publicationChannelCategory,publicSourceChannelLabel,publicSourceChannelList,publicSourceChannels} from './publicSourceChannels';

 describe('public source channel pack',()=>{
 it('contains the six required public-source surfaces',()=>{
  expect(publicSourceChannelList.map(channel=>channel.id)).toEqual(['brand_website','help_center','trusted_media_contribution','industry_community','documentation_center','public_qa_manual']);
  expect(PUBLIC_SOURCE_CHANNEL_PACK_VERSION).toMatch(/^public-source-manual-/);
 });

 it.each(publicSourceChannelList)('keeps $id manual-only with evidence and read-back guidance',channel=>{
  expect(channel).toMatchObject({category:'public_source',publishMode:'manual_only',autoPublish:false});
  expect(channel.purpose.length).toBeGreaterThan(10);
  expect(channel.requiredEvidence.length).toBeGreaterThanOrEqual(3);
  expect(channel.readBackSupport.supported).toBe(true);
  expect(channel.readBackSupport.instructions).toContain('人工');
  expect(channel.complianceReminder.length).toBeGreaterThan(10);
 });

 it('distinguishes ecommerce surfaces from public sources',()=>{
  expect(publicationChannelCategory('jd')).toBe('ecommerce');
  expect(publicationChannelCategory('xiaohongshu')).toBe('ecommerce');
  expect(publicationChannelCategory('brand_website')).toBe('public_source');
  expect(publicationChannelCategory('public_qa_manual')).toBe('public_source');
 });

 it('does not confuse media submission with publication',()=>{
  expect(publicSourceChannels.trusted_media_contribution.readBackSupport.instructions).toContain('投稿、送审或录用不等于发布');
 });

 it('requires human work and disclosed identity for public QA',()=>{
  const qa=publicSourceChannels.public_qa_manual;
  expect(qa.label).toContain('人工');
  expect(qa.requiredEvidence.join('')).toContain('答主身份');
  expect(qa.complianceReminder).toContain('禁止机器批量作答');
 });

 it('provides safe lookup helpers without accepting arbitrary ids',()=>{
  expect(isPublicSourceChannel('help_center')).toBe(true);
  expect(isPublicSourceChannel('jd')).toBe(false);
  expect(isPublicSourceChannel(undefined)).toBe(false);
  expect(getPublicSourceChannel('help_center')?.label).toBe('品牌帮助中心');
  expect(getPublicSourceChannel('not-a-channel')).toBeNull();
  expect(publicSourceChannelLabel(null)).toBe('公开信源');
 });
});
