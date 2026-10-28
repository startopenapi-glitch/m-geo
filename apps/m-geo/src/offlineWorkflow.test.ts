import {expect,it} from 'vitest';
import {validateOfflineCreate,validateOfflineHtml} from './offlineWorkflow';
const brand={id:'b',name:'虚构星灯',createdAt:'2026-10-28',profile:{aliases:['星灯别称'],officialUrl:'',manufacturer:'',category:'',description:'',updatedAt:''}};
const product={id:'p',brandId:'b',name:'虚构阅读灯',sku:'',category:'',officialUrl:'',sourceText:'',privateNotes:'',facts:[],revision:1,confirmedAt:'2026-10-28',updatedAt:'2026-10-28'};
const input={name:'任务',product,questionsText:'怎么选？\n如何保养？',repetitions:1,maxSamples:4};
it('requires a confirmed same-brand product and bounded neutral questions and budget',()=>{
 expect(validateOfflineCreate(brand,input)).toBe('');
 for(const change of [{product:undefined},{product:{...product,confirmedAt:null}},{product:{...product,brandId:'other'}},{name:''},{name:'字'.repeat(101)},{questionsText:''},{questionsText:Array(11).fill('怎么选？').join('\n')},{questionsText:'字'.repeat(501)},{questionsText:'怎么选？\n怎么选？'},{repetitions:0},{repetitions:4},{repetitions:1.5},{maxSamples:61},{maxSamples:3},{maxSamples:NaN}])expect(validateOfflineCreate(brand,{...input,...change})).not.toBe('');
});
it.each(['虚构星灯怎么选？','星灯别称好不好？','虚构阅读灯怎么样？'])('blocks identity in question %s',questionsText=>{expect(validateOfflineCreate(brand,{...input,questionsText})).toMatch(/问题不能包含品牌/)});
it('measures HTML UTF-8 bytes and does not treat a URL as HTML',()=>{
 expect(validateOfflineHtml('<h1>文本</h1>')).toBe('');expect(validateOfflineHtml('a'.repeat(262144))).toBe('');expect(validateOfflineHtml('a'.repeat(262145))).not.toBe('');expect(validateOfflineHtml('字'.repeat(90000))).not.toBe('');expect(validateOfflineHtml('')).not.toBe('');expect(validateOfflineHtml('https://example.invalid')).not.toBe('');
});
