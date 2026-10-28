import {fireEvent,render,screen} from '@testing-library/react';
import {describe,expect,it,vi} from 'vitest';
import GeoQualityPanel from './GeoQualityPanel';

const body='答案摘要\n虚构星野杯容量为 500 mL。\n\n事实要点\n- 容量：500 mL。\n\n来源\n虚构星野品牌《杯具规格书 DEMO-1》第 3 页。';

describe('GeoQualityPanel',()=>{
 it('describes structural gaps as advice rather than an enforced export block',()=>{
  render(<GeoQualityPanel draft={{kind:'description',body:'虚构商品的容量为500mL。'}}/>);
  expect(screen.getByText('结构待补充')).toBeInTheDocument();
  expect(screen.getByText(/结构检查仅提供修改提示，不改变原生审核或导出资格/)).toBeInTheDocument();
  expect(screen.queryByText('结构阻断')).not.toBeInTheDocument();
 });
 it('shows local review limits and permits an explicit clean-body write-back',()=>{
  const onApply=vi.fn();
  render(<GeoQualityPanel draft={{kind:'description',channel:'brand_website',body:'好的，\n'+body}} onApplyCleanedBody={onApply}/>);
  expect(screen.getByText(/仍需经过原生事实证据审核/)).toBeInTheDocument();
  expect(screen.getByText(/仅人工发布，不连接账号，不自动发布/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button',{name:'将清洗结果写回编辑框'}));
  expect(onApply).toHaveBeenCalledWith(body);
 });

 it('does not demand competitors for a single-brand comparison draft',()=>{
  render(<GeoQualityPanel draft={{kind:'comparison',channel:null,body}} intent="comparison"/>);
  expect(screen.queryByText(/缺少.*竞品/)).not.toBeInTheDocument();
  expect(screen.getByText(/对比意图本身不要求出现竞品/)).toBeInTheDocument();
 });
});
