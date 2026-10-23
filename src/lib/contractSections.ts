import type { Root, RootContent, Heading } from "mdast";

function headingText(node: Heading) {
 return node.children.map(child => "value" in child ? String(child.value) : "").join("");
}
const planHeading = /^(Spec|Goals?|Scope|Key decisions|Contracts|Tasks|Notes|规格|目标|范围|关键决策|契约|任务|备注)(?:\s*[（(].*[)）])?$/i;

/** Parse section boundaries from Markdown nodes so fenced examples remain untouched. */
export function contractSections({ streaming = false }: { streaming?: boolean } = {}) {
 return (tree: Root) => {
  function group(nodes: RootContent[], inContracts = false, live = streaming): RootContent[] {
   const result: RootContent[] = [];
   for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (node.type !== "heading") { result.push(node); continue; }
    const title = headingText(node).trim();
    const contracts = /^(Contracts|契约)(?:\s*[（(].*[)）])?$/i.test(title);
    if (!planHeading.test(title) && !(inContracts && /^C\d+(?:[.\w-]*)?(?:\s|[：:])/i.test(title))) {
     result.push(node); continue;
    }
    const body: RootContent[] = [];
    while (i + 1 < nodes.length) {
     const next = nodes[i + 1];
     if (next.type === "heading" && next.depth <= node.depth) break;
     body.push(next); i++;
    }
    const active = live && i === nodes.length - 1;
    result.push({ type: "blockquote", children: [node, ...group(body, contracts || inContracts, active)],
     data: { hName: "section", hProperties: { dataStreaming: active ? "true" : "false" } },
    } as RootContent);
   }
   return result;
  }
  tree.children = group(tree.children);
 };
}
