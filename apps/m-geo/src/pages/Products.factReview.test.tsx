import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import {setTestTransport} from '../api';
import type {Payload} from '../types';
import Products from './Products';

afterEach(()=>{cleanup();setTestTransport(undefined)});

it('saves a new fictional fact as pending and records an explicit approval decision',async()=>{
 const payloads:Payload[]=[];
 setTestTransport(async <T,>(action:string,p:Payload)=>{if(action==='product.save'){payloads.push(p);return {id:'p',...p,revision:1,confirmedAt:null} as T}throw Error(action)});
 render(<Products brand={{id:'b',name:'虚构品牌',createdAt:''}} data={{products:[],questions:[],drafts:[],publications:[],observations:[]}} refresh={vi.fn(async()=>{})}/>);
 fireEvent.click(screen.getByRole('button',{name:'新增商品'}));
 fireEvent.change(screen.getByLabelText('商品名称'),{target:{value:'虚构测试水杯'}});
 fireEvent.change(screen.getByLabelText('SKU'),{target:{value:'FICTION-CUP'}});
 fireEvent.click(screen.getByRole('button',{name:'添加事实'}));
 expect(screen.getByLabelText('事实 1 审核状态')).toHaveValue('pending');
 fireEvent.change(screen.getByLabelText('事实 1 名称'),{target:{value:'容量'}});
 fireEvent.change(screen.getByLabelText('事实 1 内容'),{target:{value:'500mL'}});
 fireEvent.change(screen.getByLabelText('事实 1 依据'),{target:{value:'虚构规格表'}});
 expect(screen.getByLabelText('事实 1 审核状态')).toHaveValue('pending');
 fireEvent.change(screen.getByLabelText('事实 1 审核状态'),{target:{value:'approved'}});
 fireEvent.click(screen.getByRole('button',{name:'保存商品'}));
 await waitFor(()=>expect(payloads).toHaveLength(1));
 expect(payloads[0].facts).toEqual([expect.objectContaining({label:'容量',value:'500mL',source:'虚构规格表',reviewStatus:'approved',reviewedAt:null})]);
});
