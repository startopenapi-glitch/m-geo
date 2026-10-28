import {render,screen,fireEvent,waitFor} from '@testing-library/react';
import {describe,it,expect,vi} from 'vitest';
import SiteExport from './SiteExport';
import CommerceDashboard from './CommerceDashboard';
import BuyingFields from './BuyingFields';
import {call} from '../api';
import type {PageProps} from '../App';
vi.mock('../api',()=>({call:vi.fn()}));
const product={id:'p',brandId:'b',name:'虚构水杯',sku:'X',category:'杯',officialUrl:'',sourceText:'',privateNotes:'',facts:[{id:'f',label:'容量',value:'500mL',source:'虚构依据',visibility:'public' as const,validUntil:null}],revision:1,confirmedAt:'2026-10-23T00:00:00Z',updatedAt:'2026-10-23T00:00:00Z'};
const props:PageProps={brand:{id:'b',name:'虚构品牌',createdAt:''},data:{products:[product],questions:[],observations:[],publications:[],drafts:[{id:'d',brandId:'b',productId:'p',questionId:null,kind:'faq',title:'虚构 FAQ',body:'虚构正文',claims:[{id:'c',text:'虚构正文',factId:'f'}],sourceRevision:1,revision:2,generationMode:'local',reviewedAt:'2026-10-23T00:00:00Z',createdAt:''}]},refresh:async()=>{}};
describe('commerce UI',()=>{
 it('saves a reviewed site with pinned revisions and reports cancellation honestly',async()=>{vi.mocked(call).mockResolvedValueOnce(null);render(<SiteExport {...props}/>);fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByText('导出网站 ZIP'));await screen.findByText('已取消保存网站包');expect(call).toHaveBeenCalledWith('file.exportSite',{brandId:'b',baseUrl:'',drafts:[{id:'d',expectedRevision:2}]});});
 it('does not offer stale drafts for website export',()=>{render(<SiteExport {...props} data={{...props.data,products:[{...product,revision:2}]}}/>);expect(screen.getByText('导出网站 ZIP')).toBeDisabled();});
 it('preserves private buying facts when editing a different field',()=>{const onChange=vi.fn();render(<BuyingFields facts={[{id:'commerce:price',label:'价格',value:'10',source:'内部',visibility:'private',validUntil:null}]} onChange={onChange}/>);fireEvent.change(screen.getByLabelText('购买档案 · 价格'),{target:{value:'12'}});expect(onChange.mock.calls[0][0][0].visibility).toBe('private');});
 it('distinguishes measured no recommendations from unassessed results',()=>{const o={id:'o',productId:'p',surface:'consumer',assessment:{campaignId:'c',recommendation:'no'}};render(<CommerceDashboard data={{...props.data,observations:[o as never],campaigns:[{id:'c',evidenceMode:'recorded'} as never]}}/>);expect(screen.getByText('已核对，暂无购买推荐')).toBeInTheDocument();});
});
