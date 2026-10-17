import { invoke } from '@tauri-apps/api/core';
import type { Payload } from './types';
export type Transport = <T>(action:string, payload:Payload)=>Promise<T>;
let testTransport: Transport | undefined;
/** Explicit test harness only; production never silently substitutes a mock database. */
export function setTestTransport(transport:Transport|undefined) {
  if (!import.meta.env.DEV && import.meta.env.MODE !== 'test') throw new Error('仅测试环境允许替换传输');
  testTransport=transport;
}
export async function call<T=unknown>(action:string,payload:Payload={}):Promise<T> {
  if (testTransport) return testTransport<T>(action,payload);
  if (!('__TAURI_INTERNALS__' in window)) throw new Error('请在 M- GEO 桌面应用中操作。本页为界面预览，不会保存业务数据。');
  try { return await invoke<T>('dispatch',{action,payload}); }
  catch(error) { throw new Error(typeof error==='string'?error:'操作未完成，请重试'); }
}
