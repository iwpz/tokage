// Offline production Markdown component check; never controls the application WebView.
import {pathToFileURL} from 'node:url';
import {mkdir,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const [modulePath,output]=process.argv.slice(2);
if(!modulePath||!output)throw new Error('Pass external Playwright module path and result directory');
const {webkit}=await import(pathToFileURL(modulePath).href);
await mkdir(output,{recursive:true});const browser=await webkit.launch();const results=[];
try{for(const width of [1280,640,420]){
 const page=await browser.newPage({viewport:{width,height:700},reducedMotion:'reduce'});
 await page.goto('http://127.0.0.1:1533/tests/visual/markdown-stream.html');
 const c1=page.getByRole('button',{name:'C1 Public API',exact:true});await c1.click();
 const node=await c1.elementHandle();assert.equal(await c1.getAttribute('aria-expanded'),'true');
 await page.getByRole('button',{name:'Append stream',exact:true}).click();
 await page.getByRole('button',{name:'C2 File scope',exact:true}).waitFor();
 assert(await node.evaluate(el=>el.isConnected),'stream remounted the contract card');
 assert.equal(await c1.getAttribute('aria-expanded'),'true','stream collapsed the open contract');
 const cards=page.locator('section');assert.equal(await cards.count(),2);
 assert.equal(await cards.first().locator('section').count(),0,'C2 nested in C1');
 assert(await cards.first().getByText('API acceptance criteria.',{exact:true}).isVisible());
 assert.equal(await cards.first().getByRole('button',{name:'C2 File scope',exact:true}).count(),0);
 assert(await cards.first().getByRole('button',{name:'Copy',exact:true}).isVisible(),'code copy action lost');
 await page.screenshot({path:`${output}/contracts-stream-${width}.png`});results.push({width,passed:true});await page.close();
}await writeFile(`${output}/results.json`,JSON.stringify({evidenceClass:'offline-component-webkit',productUiAcceptance:false,results},null,2)+'\n');console.log(JSON.stringify(results));}finally{await browser.close();}
