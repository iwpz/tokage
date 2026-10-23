// Offline presentation fixture. No App state, transport, database, or model calls.
import {useState} from "react";
import {createRoot} from "react-dom/client";
import "../../src/index.css";
import i18n from "../../src/i18n";
import {Markdown} from "../../src/components/Markdown";
await i18n.changeLanguage("en");
const initial="# Contracts\n\n## C1 Public API\n\n```ts\nexport interface Contract {\n name: string;";
function Fixture(){const [stream,setStream]=useState(false);return <main className="h-screen overflow-y-auto p-5"><p className="mb-4 text-xs text-muted">Offline Markdown stream fixture — no model calls</p><button className="mb-4 rounded border border-border px-3 py-1 text-xs" onClick={()=>setStream(true)}>Append stream</button><Markdown text={initial+(stream?"\n value: number;\n}\n```\n\nAPI acceptance criteria.\n\n## C2 File scope\n\nOnly source may change.\n\n# Tasks\n\nImplementation follows.":"")} onOpenFile={()=>{}}/></main>;}
createRoot(document.getElementById("root")!).render(<Fixture/>);
