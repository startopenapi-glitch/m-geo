import {useState} from 'react';
import {call} from '../api';
import {Field,useTask} from './ui';
interface Inspection {valid:true;sourceVersion:number;targetVersion:number;counts:{brands:number;products:number;datasets:number;localJobs:number;optimizationPlans:number;offlineWorkflows?:number;p0Workflows?:number};workspaceUnchanged:true}
export default function BackupInspector({disabled=false}:{disabled?:boolean}){
 const [password,setPassword]=useState(''),[result,setResult]=useState<Inspection|null>(null);const task=useTask();
 function inspect(){const secret=password;setPassword('');setResult(null);void task.run(()=>call<Inspection|null>('backup.inspect',{password:secret}),setResult,r=>r?'校验完成，当前工作区未替换':'已取消校验，当前工作区未替换')}
 return <section aria-label="备份只读演练"><h3>备份只读演练</h3><p>在临时副本中解密、升级和检查备份，核对记录数量。当前工作区未替换，模型密钥不会从备份恢复。</p><Field label="校验备份口令" hint="选择文件前立即清空口令输入。"><input type="password" autoComplete="off" disabled={disabled||task.busy} value={password} maxLength={256} onChange={e=>setPassword(e.target.value)}/></Field><button type="button" disabled={disabled||task.busy||!password} onClick={inspect}>选择备份并只读校验</button>{task.feedback}{result&&<div className="notice" role="status"><strong>备份校验通过</strong><p>源版本 {result.sourceVersion} → 当前版本 {result.targetVersion}</p><p>品牌 {result.counts.brands} · 商品 {result.counts.products} · 归因数据集 {result.counts.datasets} · 本地队列 {result.counts.localJobs} · 优化计划 {result.counts.optimizationPlans} · 离线任务 {result.counts.offlineWorkflows??0} · 接入演练 {result.counts.p0Workflows??0}</p><p>当前工作区未替换。通过校验说明此备份可被当前版本读取，不代表已经恢复。</p></div>}</section>
}
