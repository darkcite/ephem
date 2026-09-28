const http=require('http'),fs=require('fs'),path=require('path');const {chromium}=require('playwright-core');
const srv=http.createServer((q,r)=>{if(q.url==='/page.js'){r.writeHead(200,{'content-type':'text/javascript'});r.end(fs.readFileSync(path.join(__dirname,'page.js')))}else{r.writeHead(200,{'content-type':'text/html'});r.end('<!doctype html><script src="/page.js"></script>')}});
async function run(name,args,perm,cam){
  const b=await chromium.launch({executablePath:'/opt/pw-browsers/chromium-1194/chrome-linux/chrome',args});
  const url=`http://127.0.0.1:${srv.address().port}`;const c=await b.newContext();if(perm)await c.grantPermissions(['camera'],{origin:url});
  const p=await c.newPage();await p.goto(url+'/');
  let camres=null; if(cam) camres=await p.evaluate(()=>T.cameraThenStop().then(()=> 'ok',e=>'err:'+e.name));
  const o=await p.evaluate(()=>T.offer());
  // second context (no permission) to test the mDNS connection path
  const c2=await b.newContext();const p2=await c2.newPage();await p2.goto(url+'/');
  const a=await p2.evaluate(f=>T.answer(f),o);await p.evaluate(f=>T.applyAnswer(f),a);
  const st=await Promise.all([p.evaluate(()=>T.waitOpen(15000)),p2.evaluate(()=>T.waitOpen(15000))]);
  console.log(name,'| camera:',camres,'| alice cands',JSON.stringify(o.cands.map(x=>x.addr)),'| bob cands',JSON.stringify(a.cands.map(x=>x.addr)),'| channel',st.join('/'));
  await b.close();
}
(async()=>{await new Promise(r=>srv.listen(0,'127.0.0.1',r));
 await run('A no flags, no permission',[],false,false);
 await run('B permission granted, camera never opened',['--use-fake-device-for-media-stream'],true,false);
 await run('C permission granted, camera opened+stopped',['--use-fake-device-for-media-stream'],true,true);
 await run('D fake-ui flag (auto-accept prompts)',['--use-fake-ui-for-media-stream'],false,false);
 srv.close();})().catch(e=>{console.error(e);process.exit(1)});
