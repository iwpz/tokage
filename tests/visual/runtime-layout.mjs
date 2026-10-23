// Offline visual regression only. Requires an explicitly started isolated component fixture server.
import { writeFile, mkdir } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
const [modulePath, outputDir] = process.argv.slice(2);
if (!modulePath || !outputDir) throw new Error('Usage: node tests/visual/runtime-layout.mjs /absolute/playwright/index.mjs /absolute/output');
const { webkit } = await import(pathToFileURL(modulePath).href);
await mkdir(outputDir, { recursive: true });
const browser = await webkit.launch({ headless: true });
const results = [];
let activePage;
try {
 for (const viewport of [{width:1280,height:820},{width:1024,height:700},{width:640,height:480},{width:420,height:480}]) {
  for (const view of ['plan','pipeline']) {
   const page = await browser.newPage({ viewport }); activePage=page;
   const errors = []; page.on('pageerror', e => errors.push(String(e)));
   await page.goto(`http://127.0.0.1:1533/tests/visual/runtime-layout.html?view=${view}`);
   await page.getByText('Offline component layout fixture — no application data or model calls').waitFor();
   await page.evaluate(() => document.documentElement.classList.add('dark'));
   const within = async locator => { const box = await locator.boundingBox(); assert(box && box.height > 0 && box.x >= 0 && box.x + box.width <= viewport.width + 1 && box.y >= 0 && box.y + box.height <= viewport.height + 1, `${view} content outside viewport: ${JSON.stringify(box)}`); const visibleHeight=await locator.evaluate(el=>{const r=el.getBoundingClientRect();let top=Math.max(0,r.top),bottom=Math.min(innerHeight,r.bottom);for(let p=el.parentElement;p;p=p.parentElement){if(['hidden','auto','scroll','clip'].includes(getComputedStyle(p).overflowY)){const b=p.getBoundingClientRect();top=Math.max(top,b.top);bottom=Math.min(bottom,b.bottom);}}return Math.max(0,bottom-top);}); assert(visibleHeight>=box.height-1,`${view} element clipped by an ancestor`); return box; };
   const root = await page.evaluate(() => ({width:innerWidth,height:innerHeight,scrollWidth:document.documentElement.scrollWidth,scrollHeight:document.documentElement.scrollHeight}));
   assert(root.scrollWidth <= root.width + 1, 'horizontal document overflow');
   assert(root.scrollHeight <= root.height + 1, 'vertical document overflow');
   let content;
   if (view === 'plan') {
    await page.getByText('Structured feature goal', {exact:true}).waitFor();
    content = await within(page.getByTestId('runtime-plan-body'));
    assert(content.height >= viewport.height * .35, 'plan body squeezed by toolbars');
    await within(page.getByText('Structured feature goal',{exact:true}));
    await within(page.getByRole('button',{name:'Edit spec',exact:true}));
    await page.getByText('Preserve API',{exact:true}).scrollIntoViewIfNeeded(); await within(page.getByText('Preserve API',{exact:true}));
    await page.getByRole('tab',{name:/Contracts/}).click(); await within(page.getByText('API contract',{exact:true}));
    await page.getByRole('tab',{name:/Tasks/}).click(); await within(page.getByText('Implement feature',{exact:true}));
    await page.getByRole('tab',{name:'Spec',exact:true}).click();
   } else {
    content = await within(page.getByRole('region',{name:'Technical plan sub-agent execution board'}));
    await within(page.getByText('Implement feature',{exact:true}));
    await page.getByRole('button',{name:'Execution settings',exact:true}).click();
    await page.getByRole('dialog').waitFor();
    assert.deepEqual(await page.getByRole('region',{name:'Technical plan sub-agent execution board'}).boundingBox(),content,'settings shifted task board');
    await page.getByRole('button',{name:'Close',exact:true}).click();
   }
   await page.getByRole('button',{name:'Review and approve',exact:true}).click();
   const approve=page.getByRole('button',{name:'Approve plan with Full Auto',exact:true});
   assert(await approve.isDisabled(),'approval enabled before exact commands');
   await page.getByLabel('npm test',{exact:true}).check(); assert(await approve.isEnabled(),'approval not enabled after exact command');
   await page.getByRole('button',{name:'Close',exact:true}).click();
   assert.deepEqual(errors,[],'browser errors');
   await page.screenshot({path:`${outputDir}/${view}-${viewport.width}x${viewport.height}.png`});
   results.push({view,viewport,root,content,passed:true});
   await page.close();
  }
 }
 await writeFile(`${outputDir}/results.json`,JSON.stringify({evidenceClass:'offline-component-webkit-layout',productUiAcceptance:false,results},null,2)+'\n');
 console.log(JSON.stringify({passed:results.length,outputDir}));
} catch (error) {
 if(activePage && !activePage.isClosed()) await activePage.screenshot({path:`${outputDir}/failure.png`});
 await writeFile(`${outputDir}/failure.json`,JSON.stringify({error:String(error),results},null,2));
 throw error;
} finally { await browser.close(); }
