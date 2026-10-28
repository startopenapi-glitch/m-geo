import {expect,it} from 'vitest';
import config from '../src-tauri/tauri.conf.json';

it('allows local BLOB previews in the packaged desktop policy without allowing remote media or BLOB scripts',()=>{
 const directives=new Map(config.app.security.csp.split(';').map(part=>{
  const [name,...sources]=part.trim().split(/\s+/);
  return [name,sources];
 }));
 const sources=(kind:string)=>directives.get(kind)??directives.get('default-src')??[];
 expect(sources('img-src')).toEqual(["'self'",'asset:','http://asset.localhost','data:','blob:']);
 expect(sources('media-src')).toEqual(["'self'",'blob:']);
 expect(sources('script-src')).not.toContain('blob:');
 expect(sources('connect-src')).not.toContain('blob:');
 for(const kind of ['img-src','media-src']){
  expect(sources(kind)).not.toContain('*');
  expect(sources(kind)).not.toContain('https:');
 }
});
