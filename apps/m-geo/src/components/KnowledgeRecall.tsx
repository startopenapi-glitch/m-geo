import {useEffect,useRef,useState,type KeyboardEvent} from 'react';
import {Search} from 'lucide-react';
import {call} from '../api';
import type {KnowledgeRecallHit} from '../knowledgeTypes';
import {Badge,Field,time} from './ui';

const riskLabels={low:'低风险',medium:'中风险',high:'高风险'} as const;

export default function KnowledgeRecall({brandId,productId}:{brandId:string;productId:string}){
 const [query,setQuery]=useState(''),[hits,setHits]=useState<KnowledgeRecallHit[]>([]),[error,setError]=useState(''),[busy,setBusy]=useState(false),[lastQuery,setLastQuery]=useState<string|null>(null);
 const request=useRef(0),alive=useRef(true);
 useEffect(()=>{alive.current=true;request.current++;setQuery('');setHits([]);setError('');setBusy(false);setLastQuery(null);return()=>{alive.current=false;request.current++}},[brandId,productId]);
 async function search(){
  const value=query.trim(),sequence=++request.current;
  if(!value){setError('请输入要检索的关键词');return}
  setBusy(true);setError('');
  try{
   const results=await call<KnowledgeRecallHit[]>('knowledge.recall',{brandId,productId,query:value,limit:20});
   if(!alive.current||sequence!==request.current)return;
   setHits(results);setLastQuery(value);
  }catch(reason){
   if(alive.current&&sequence===request.current)setError(reason instanceof Error?reason.message:'知识召回未完成，请重试');
  }finally{
   if(alive.current&&sequence===request.current)setBusy(false);
  }
 }
 function onKeyDown(event:KeyboardEvent<HTMLInputElement>){if(event.key==='Enter'){event.preventDefault();if(query.trim())void search()}}
 return <section aria-label="知识召回（只读）">
 <div className="subheading"><h3>知识召回（只读）</h3><Badge tone="blue">本地关键词 · 已审核知识</Badge></div>
 <p className="inline-note">仅检索当前品牌已审核的通用知识与当前商品知识。不是 AI 语义检索；不会自动插入正文、生成事实、批准稿件或连接外部平台。</p>
 <div className="actions"><Field label="知识召回关键词" hint="中文按子串匹配，英文与数字按空白分词；最多 200 字，返回最多 20 条。"><input value={query} maxLength={200} onChange={event=>setQuery(event.target.value)} onKeyDown={onKeyDown} placeholder="例如：容量 500"/></Field><button type="button" disabled={!query.trim()} onClick={()=>void search()}><Search size={15}/>{busy?'检索中…':'检索已审核知识'}</button></div>
 {error&&<p role="alert" className="feedback error">{error}；已保留上次成功结果，可修改关键词后重试。</p>}
 {lastQuery!==null&&<p className="inline-note">上次成功检索：{lastQuery} · {hits.length} 条</p>}
 {lastQuery!==null&&!hits.length&&!error&&<p className="inline-note">未找到同时匹配全部关键词的已审核知识切片。</p>}
 <div className="draft-list">{hits.map(hit=><article className="fact-card" data-testid="knowledge-recall-hit" key={`${hit.documentId}-${hit.chunkId}`}>
 <div className="subheading"><strong>{hit.documentTitle}</strong><Badge tone={hit.risk==='high'?'amber':'neutral'}>{riskLabels[hit.risk]}</Badge></div>
 <p>{hit.text}</p>
 <p className="inline-note">来源：{hit.source} · 文档审核版本 v{hit.documentRevision} · 切片 #{hit.chunkOrdinal+1} · 审核于 {time(hit.reviewedAt)}{hit.effectiveAt?` · 生效日期 ${hit.effectiveAt}`:''}</p>
 <code>SHA-256 {hit.sha256}</code>
 </article>)}</div>
 </section>
}
