import {useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {Panel,useTask} from '../components/ui';
import SitePackageInspector from '../components/SitePackageInspector';

interface Preparation {
 token:string;
 products:{id:string;name:string;gaps:string[];reviewedDraftCount:number}[];
 candidates:{productId:string;text:string;intent:string}[];
}

export default function TrialPreparation({brand,data,refresh}:PageProps){
 const [ids,setIds]=useState<string[]>([]);
 const [report,setReport]=useState<{key:string;value:Preparation}|null>(null);
 const task=useTask();
 const payload={brandId:brand.id,productIds:ids};
 const key=JSON.stringify({payload,data});
 const current=useRef(key);
 current.current=key;
 const visible=report?.key===key?report.value:null;
 const products=data.products.filter(p=>p.brandId===brand.id);

 return <>
  <div className="page-heading">
   <div>
    <h1>试点准备</h1>
    <p>整理1至3个商品的公开资料与十条候选问题。</p>
   </div>
  </div>
  <div className="notice">
   本地整理 · 尚未发布。模板建议不是真实搜索需求，不能证明收录、公开可达或推荐。网站与渠道授权需要外部确认。
  </div>
  <Panel title="选择试点商品" description="准备报告不会生成答案或自动保存问题。">
   {products.slice(0,500).map(p=>(
    <label className="check" key={p.id}>
     <input
      type="checkbox"
      checked={ids.includes(p.id)}
      disabled={task.busy||(!ids.includes(p.id)&&ids.length>=3)}
      onChange={e=>{
       setIds(old=>e.target.checked
        ? [...old,p.id]
        : old.filter(id=>id!==p.id));
       setReport(null);
      }}
     />
     {p.name} · {p.sku}
    </label>
   ))}
   <div className="actions">
    <button
     disabled={task.busy||!ids.length}
     onClick={()=>void task.run(
      ()=>call<Preparation>('pilot.prepare',payload),
      value=>{
       if(current.current===key){
        setReport({key,value});
       }
      },
      '本地准备完成'
     )}
    >
     准备试点资料
    </button>
    <button
     disabled={task.busy||!visible}
     onClick={()=>void task.run(
      ()=>call('file.exportPilot',payload),
      undefined,
      result=>result?'试点 ZIP 已保存到本机；尚未发布':'已取消导出'
     )}
    >
     导出试点 ZIP
    </button>
    <button
     disabled={task.busy||!visible}
     onClick={()=>void task.runPersisted(
      ()=>call<{created:number;skipped:number}>('pilot.createQuestions',{
       ...payload,
       token:visible?.token,
       confirm:true
      }),
      refresh,
      ()=>setReport(null),
      result=>`候选问题处理完成：新增 ${result.created} 条待确认问题，跳过 ${result.skipped} 条已有问题；已有问题的确认状态保持不变。${result.created?'请到商品资料确认新增问题后再创建实验。':''}`
     )}
    >
     保存候选问题（未确认）
    </button>
   </div>
   {task.feedback}
  </Panel>
  {visible&&(
   <Panel title="本地试点报告" description="请稍后在商品资料中逐条确认候选问题，再创建实验。">
    {visible.products.map(p=>(
     <article key={p.id}>
      <h3>{p.name}</h3>
      <p>当前已审核稿件：{p.reviewedDraftCount}</p>
      <ul>
       {p.gaps.map(gap=><li key={gap}>{gap}</li>)}
      </ul>
     </article>
    ))}
    <ol>
     {visible.candidates.slice(0,10).map((q,i)=><li key={i}>{q.text}</li>)}
    </ol>
   </Panel>
  )}
  <SitePackageInspector brandId={brand.id} data={data}/>
 </>;
}
