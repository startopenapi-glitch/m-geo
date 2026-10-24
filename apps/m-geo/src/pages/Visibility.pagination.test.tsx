import {fireEvent,render,screen,within} from '@testing-library/react';
import {it,expect,vi} from 'vitest';
import Visibility from './Visibility';
import type {PageProps} from '../App';
function fixture(n=45):PageProps{return {brand:{id:'fixture',name:'虚构品牌'},refresh:vi.fn(),data:{products:[],questions:[],drafts:[],publications:[],campaigns:[],observations:Array.from({length:n},(_,i)=>({id:`o${i}`,productId:'p',questionId:`q${i}`,querySnapshot:`虚构问题${i}`,platform:i<40?'doubao':'yuanbao',surface:'consumer',modelLabel:'fixture',searchMode:'off',branded:false,phase:'baseline',testedAt:'2026-10-23T00:00:00Z',answer:`虚构回答${i}`,mentioned:false,accuracy:'unknown',citedUrl:'',evidenceNote:'',assessment:{recommendation:'unknown',context:'fixture',repetition:1}}))}} as unknown as PageProps}
it('bounds groups/raw records, navigates last/previous, preserves full totals and clamps shrink/filter',()=>{
 const p=fixture();const view=render(<Visibility {...p}/>);
 expect(view.container.querySelectorAll('.observation').length).toBe(20);
 fireEvent.click(view.container.querySelector('.observation summary')!);
 fireEvent.click(screen.getAllByText('核对 / 更正评估')[0]);expect(screen.getByText('保存评估更正')).toBeTruthy();
 const groups=screen.getByRole('navigation',{name:'样本分组分页'}),raw=screen.getByRole('navigation',{name:'原始记录分页'});
 expect(screen.getByText('人工实测 0 · 模拟 0 · 未分类 40')).toBeTruthy();
 expect(screen.queryByRole('heading',{name:'虚构问题10'})).toBeNull();
 for(let i=0;i<4;i++)fireEvent.click(within(groups).getByText('下一页'));
 expect(within(groups).getByText('第 5/5 页 · 共 45 组')).toBeTruthy();
 fireEvent.click(within(groups).getByText('上一页'));expect(within(groups).getByText('第 4/5 页 · 共 45 组')).toBeTruthy();
 fireEvent.click(within(raw).getByText('下一页'));expect(screen.queryByText('保存评估更正')).toBeNull();fireEvent.click(within(raw).getByText('下一页'));
 expect(within(raw).getByText('第 3/3 页 · 共 45 条')).toBeTruthy();fireEvent.click(within(raw).getByText('上一页'));
 expect(within(raw).getByText('第 2/3 页 · 共 45 条')).toBeTruthy();
 view.rerender(<Visibility {...fixture(2)}/>);expect(within(groups).getByText('第 1/1 页 · 共 2 组')).toBeTruthy();expect(within(raw).getByText('第 1/1 页 · 共 2 条')).toBeTruthy();
 view.rerender(<Visibility {...p}/>);fireEvent.click(within(groups).getByText('下一页'));fireEvent.click(within(raw).getByText('下一页'));
 fireEvent.change(screen.getByLabelText('筛选实测平台'),{target:{value:'yuanbao'}});
 expect(within(groups).getByText('第 1/1 页 · 共 5 组')).toBeTruthy();expect(within(raw).getByText('第 1/1 页 · 共 5 条')).toBeTruthy();
 view.rerender(<Visibility {...fixture(2)}/>);expect(within(raw).getByText('第 1/1 页 · 共 0 条')).toBeTruthy();
});
