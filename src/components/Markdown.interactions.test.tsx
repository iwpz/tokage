// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import "../i18n";
import { Markdown } from "./Markdown";
const contract="# Contracts\n## C1 API\n```ts\nexport function format(ms: number): string;\n```\nAPI criteria\n\n## C2 Files\nOnly source may change.\n\n# Tasks\nOther content";
it("renders sibling contract cards and retains open state while text streams",async()=>{
 (globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host);
 try {
  const render=async(text:string)=>act(async()=>{root.render(<Markdown text={text} onOpenFile={()=>{}}/>);});
  await render(contract);const buttons=()=>[...host.querySelectorAll<HTMLButtonElement>('section > button')];
  expect(buttons().map(b=>b.textContent)).toEqual(['C1 API','C2 Files']);
  await act(async()=>buttons()[0].click());expect(buttons()[0].getAttribute('aria-expanded')).toBe('true');
  const first=host.querySelector('section')!;expect(first.querySelectorAll('section').length).toBe(0);expect(first.querySelector('pre')?.textContent).toContain('export function');
  expect(first.textContent).toContain('API criteria');expect(first.textContent).not.toContain('C2 Files');
  await render(contract+'\nMore streamed content');expect(buttons()[0].getAttribute('aria-expanded')).toBe('true');
  await act(async()=>buttons()[0].click());await render(contract+'\nEven more');expect(buttons()[0].getAttribute('aria-expanded')).toBe('false');
 }finally{await act(async()=>root.unmount());host.remove();}
});
it("keeps standalone fenced blocks expanded through stream updates and changing callbacks",async()=>{
 const host=document.createElement('div');const root=createRoot(host);(globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;
 try {
  const render=async(text:string)=>act(async()=>root.render(<Markdown text={text} onOpenFile={()=>{}}/>));
  const text='```ts\ninterface API {\n a: string;\n b: string;\n}\n```';await render(text);
  await act(async()=>host.querySelector<HTMLButtonElement>('button[title]')!.click());
  expect(host.querySelector('pre')).toBeTruthy();await render(text+'\nstreamed text');expect(host.querySelector('pre')).toBeTruthy();
 }finally{await act(async()=>root.unmount());}
});
it("does not treat Contracts headings inside code fences as document sections",async()=>{
 const host=document.createElement('div');const root=createRoot(host);
 try{await act(async()=>root.render(<Markdown text={'```md\n# Contracts\n## C1 not a heading\n```'}/>));expect(host.querySelector('section')).toBeNull();}
 finally{await act(async()=>root.unmount());}
});
