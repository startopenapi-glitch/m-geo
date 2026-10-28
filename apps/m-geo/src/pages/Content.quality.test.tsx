import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import type {Draft,Product,WorkspaceSnapshot} from '../types';
import {DraftEditor} from './Content';

const product:Product={id:'p',brandId:'b',name:'虚构水杯',sku:'SIM-CUP',category:'测试',officialUrl:'',sourceText:'',privateNotes:'',revision:1,confirmedAt:'2026-10-24T00:00:00Z',updatedAt:'',facts:[{id:'capacity',label:'容量',value:'500mL',source:'虚构规格表第 2 页',visibility:'public',validUntil:null,reviewStatus:'approved',reviewedAt:'2026-10-24T00:00:00Z',updatedAt:'2026-10-24T00:00:00Z'}]};
const draft:Draft={id:'d',brandId:'b',productId:'p',questionId:null,kind:'description',title:'虚构介绍',body:'容量：500mL',claims:[{id:'c',text:'容量：500mL',factId:'capacity'}],sourceRevision:1,revision:1,generationMode:'local',reviewedAt:null,createdAt:''};
const data:WorkspaceSnapshot={products:[product],questions:[],drafts:[draft],publications:[],observations:[]};
const props={brand:{id:'b',name:'虚构品牌',createdAt:''},data,refresh:vi.fn(async()=>{})};
afterEach(cleanup);

it('shows evidence coverage and keeps local rule success distinct from human review',()=>{
 render(<DraftEditor {...props} draft={draft}/>);
 expect(screen.getByText('规则通过')).toBeInTheDocument();
 expect(screen.getByText(/关键主张 1 条，已关联有来源的已审核公开事实 1 条/)).toBeInTheDocument();
 expect(screen.getByText(/不代表事实已核实或内容已获准发布/)).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'导出已审核内容'})).toBeDisabled();
});

it('updates quality findings from unsaved editor changes without replacing the native review gate',()=>{
 render(<DraftEditor {...props} draft={draft}/>);
 fireEvent.change(screen.getByLabelText('依据事实 1'),{target:{value:''}});
 expect(screen.getByText('存在阻断')).toBeInTheDocument();
 expect(screen.getByText('关键主张关联的事实不存在，请重新选择当前商品的事实依据。')).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'确认内容并审核'})).toBeDisabled();
 fireEvent.change(screen.getByLabelText('依据事实 1'),{target:{value:'capacity'}});
 fireEvent.change(screen.getByLabelText('草稿正文'),{target:{value:'容量：500mL。保证一天见效'}});
 expect(screen.getByText(/正文含功效或结果承诺/)).toBeInTheDocument();
 expect(screen.getByRole('button',{name:'保存修改'})).toBeEnabled();
});
