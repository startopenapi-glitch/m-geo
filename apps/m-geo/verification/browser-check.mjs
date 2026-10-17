// Real Chromium UI -> test-only JSON stdio -> actual Rust commands + isolated SQLite.
// Does not exercise the Tauri IPC protocol or native dialogs; those require desktop acceptance.
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
const require=createRequire(import.meta.url);
const {chromium}=require(process.env.MGEO_PLAYWRIGHT||'playwright');
const root=path.resolve(import.meta.dirname,'..'),evidence=path.join(root,'verification','screenshots');
await mkdir(evidence,{recursive:true}); const temp=await mkdtemp(path.join(os.tmpdir(),'mgeo-e2e-'));
const child=spawn(path.join(root,'src-tauri/target/debug/examples/core_bridge'),[path.join(temp,'test.sqlite3')],{stdio:['pipe','pipe','inherit']});
const queue=[];createInterface({input:child.stdout}).on('line',line=>{const r=JSON.parse(line),q=queue.shift();r.ok?q.resolve(r.value):q.reject(new Error(r.error))});
const call=(action,payload={})=>new Promise((resolve,reject)=>{queue.push({resolve,reject});child.stdin.write(JSON.stringify({action,payload})+'\n')});
const browser=await chromium.launch({headless:true,...(process.env.MGEO_CHROMIUM?{executablePath:process.env.MGEO_CHROMIUM}:{})}),page=await browser.newPage({viewport:{width:1280,height:850},locale:'zh-CN',timezoneId:'Asia/Shanghai'});
const errors=[],checks=[];page.on('pageerror',e=>errors.push(e.message));
await page.exposeFunction('nativeTest',call);
await page.addInitScript(()=>{window.__TAURI_INTERNALS__={invoke:(_command,{action,payload})=>window.nativeTest(action,payload)}});
const nav=label=>page.getByRole('navigation').getByRole('button',{name:label,exact:true}).click();
const expectVisible=async text=>{await page.getByText(text,{exact:true}).first().waitFor();checks.push(text)};
try {
 await page.goto(process.env.MGEO_TEST_URL||'http://127.0.0.1:1420');
 await page.getByLabel('品牌名称',{exact:true}).fill('验收品牌 A');await page.getByRole('button',{name:'创建品牌',exact:true}).click();
 await expectVisible('今天，从准确的资料开始');
 await nav('商品资料');await page.getByRole('button',{name:'新增商品',exact:true}).click();
 await page.getByLabel('商品名称',{exact:true}).fill('测试保温杯');await page.getByLabel('SKU',{exact:true}).fill('CUP-001');
 await page.getByLabel('类目',{exact:true}).fill('家居');await page.getByRole('button',{name:'添加事实',exact:true}).click();
 await page.getByLabel('事实 1 名称',{exact:true}).fill('容量');await page.getByLabel('事实 1 内容',{exact:true}).fill('500mL');await page.getByLabel('事实 1 依据',{exact:true}).fill('厂家参数表，仅作测试');
 await page.getByRole('button',{name:'保存商品',exact:true}).click();await page.getByRole('button',{name:'确认已保存的事实',exact:true}).click();
 await page.getByRole('button',{name:'添加模板建议',exact:true}).click();await page.getByRole('button',{name:'确认问题',exact:true}).first().click();
 await nav('内容工作室');await page.getByLabel('用于生成的商品',{exact:true}).selectOption({label:'测试保温杯'});
 await page.getByRole('button',{name:'生成本地草稿',exact:true}).click();await page.getByLabel('草稿正文',{exact:true}).waitFor();
 assert.match(await page.getByLabel('草稿正文',{exact:true}).inputValue(),/500mL/);
 await page.getByRole('button',{name:'确认内容并审核',exact:true}).click();await expectVisible('当前已审核');
 const brands=await call('brand.list'),brand=brands[0],snapshot=await call('workspace.snapshot',{brandId:brand.id});
 const draft=snapshot.drafts[0]; const exported=await call('draft.export',{brandId:brand.id,id:draft.id,expectedRevision:draft.revision,format:'html'});
 assert.match(exported.content,/500mL/);assert.ok(!exported.content.includes('<script'));checks.push('真实 Rust 生成、审核、安全 HTML 导出');
 await nav('发布记录');
 // Register a clearly-labelled fixture URL, never publish or contact it.
 await page.getByLabel('已审核稿件',{exact:true}).selectOption(draft.id);
 await page.getByLabel('发布渠道',{exact:true}).fill('测试记录，不是真实发布');
 await page.getByLabel('真实公开网址',{exact:true}).fill('https://example.com/mgeo-test');
 await page.getByRole('button',{name:'保存发布记录',exact:true}).click();
 await nav('AI 表现');await page.getByLabel('实测商品',{exact:true}).selectOption(snapshot.products[0].id);
 await page.getByLabel('已确认问题',{exact:true}).selectOption(snapshot.questions.find(q=>q.confirmed).id);
 await page.getByLabel('模型 / 版本标识',{exact:true}).fill('本地测试夹具');await page.getByLabel('联网 / 搜索模式',{exact:true}).fill('未联网，测试记录');
 await page.getByLabel('平台原始回答',{exact:true}).fill('这是假设回答，仅用于软件操作验收，未向平台提问。');
 await page.getByRole('button',{name:'保存实测结果',exact:true}).click();await expectVisible('实测记录已保存');
 checks.push('发布与观察登记完成，没有真实发布或请求平台');
 const final=await call('workspace.snapshot',{brandId:brand.id});assert.equal(final.publications.length,1);assert.equal(final.observations.length,1);
 assert.ok(Math.abs(Date.now()-Date.parse(final.observations[0].testedAt))<120000,'local timestamp must preserve actual instant');
 for(const width of [960,1280,1440]){
  await page.setViewportSize({width,height:width===960?640:900});
  for(const [label,slug] of [['工作台','dashboard'],['商品资料','products'],['内容工作室','content'],['发布记录','publications'],['AI 表现','visibility'],['设置','settings']]){
   await nav(label);await page.getByRole('heading',{level:1}).waitFor();
   const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1);
   assert.equal(overflow,false,`${width} ${label} horizontal overflow`);
   await page.screenshot({path:path.join(evidence,`${slug}-${width}.png`),fullPage:true});checks.push(`${width}px ${label} 无页面横向溢出`);
  }
 }
 await page.getByRole('button',{name:'添加品牌',exact:true}).click();await page.getByLabel('品牌名称',{exact:true}).fill('验收品牌 B');await page.getByRole('button',{name:'创建品牌',exact:true}).click();
 await nav('商品资料');await expectVisible('还没有商品资料');assert.equal(await page.getByText('测试保温杯',{exact:true}).count(),0);checks.push('品牌切换清空旧商品');
 const b=(await call('brand.list')).find(x=>x.name==='验收品牌 B');
 await assert.rejects(()=>call('product.confirm',{brandId:b.id,id:snapshot.products[0].id,expectedRevision:1}));checks.push('原生跨品牌对象操作拒绝');
 await page.getByLabel('当前品牌',{exact:true}).selectOption(brand.id);await page.reload();await nav('商品资料');await page.getByText('测试保温杯',{exact:true}).first().waitFor();checks.push('重载后 SQLite 业务记录保持');
 assert.deepEqual(errors,[]);checks.push('页面无未捕获 JavaScript 错误');
 await writeFile(path.join(root,'verification/browser-results.json'),JSON.stringify({passed:true,checks,errors,testMode:'Chromium + actual Rust core + disposable SQLite; native dialogs not covered'},null,2));
 console.log(JSON.stringify({passed:true,checks:checks.length,errors}));
}catch(error){await page.screenshot({path:path.join(evidence,'failure.png'),fullPage:true});console.error(error);process.exitCode=1;}
finally{await browser.close();child.stdin.end();child.kill();await rm(temp,{recursive:true,force:true});}
