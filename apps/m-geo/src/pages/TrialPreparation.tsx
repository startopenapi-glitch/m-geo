import {useRef,useState} from 'react';
import type {PageProps} from '../App';
import {call} from '../api';
import {Field,Panel,useTask} from '../components/ui';
import SitePackageInspector from '../components/SitePackageInspector';
import {Questions} from './Products';

interface Preparation {
 token:string;
 products:{id:string;name:string;gaps:string[];reviewedDraftCount:number}[];
 candidates:{productId:string;text:string;intent:string}[];
}

export default function TrialPreparation({brand,data,refresh}:PageProps){
 const [ids,setIds]=useState<string[]>([]);
 const [questionProductId,setQuestionProductId]=useState('');
 const [report,setReport]=useState<{key:string;value:Preparation}|null>(null);
 const task=useTask();
 const payload={brandId:brand.id,productIds:ids};
 const key=JSON.stringify({payload,data});
 const current=useRef(key);
 current.current=key;
 const visible=report?.key===key?report.value:null;
 const products=data.products.filter(p=>p.brandId===brand.id);
 const questionProduct=products.find(p=>p.id===questionProductId)??products[0];

 return <>
  <div className="page-heading">
   <div>
    <h1>问题与诊断</h1>
    <p>从购买意图建立固定问题集，再核对商品资料缺口；先准确，再测试。</p>
   </div>
  </div>
  <div className="notice">
   本地整理 · 尚未发布。模板建议不是真实搜索需求，不能证明收录、公开可达或推荐。网站与渠道授权需要外部确认。
  </div>
  {questionProduct?<>
   <Panel title="固定问题集" description="按商品录入并确认消费者问题；只有已确认问题才能用于平台实测。">
    <Field label="管理哪个商品的问题"><select value={questionProduct.id} onChange={e=>setQuestionProductId(e.target.value)}>{products.map(p=><option key={p.id} value={p.id}>{p.name} · {p.sku}</option>)}</select></Field>
   </Panel>
   <Questions key={questionProduct.id} brand={brand} data={data} refresh={refresh} product={questionProduct}/>
  </>:<Panel title="固定问题集"><p>请先在“商品事实”中录入商品，再整理消费者问题。</p></Panel>}
  <Panel title="资料缺口与候选问题" description="选择1至3个商品生成只读准备报告；不会生成答案或自动保存问题。">
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
