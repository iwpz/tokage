import type { Root, RootContent, Heading } from "mdast";

function headingText(node: Heading) {
 return node.children.map(child => "value" in child ? String(child.value) : "").join("");
}
/** Group explicit Contracts/C1 headings in the parsed Markdown tree, never headings inside fences. */
export function contractSections() {
 return (tree: Root) => {
  const result: RootContent[]=[];
  let contractsDepth: number | null=null;
  for(let i=0;i<tree.children.length;i++) {
   const node=tree.children[i];
   if(node.type==="heading") {
    const title=headingText(node).trim();
    if(contractsDepth!==null && node.depth<=contractsDepth) contractsDepth=null;
    if(/^(Contracts|契约)(?:\s*[（(].*[)）])?$/i.test(title)) contractsDepth=node.depth;
    else if(contractsDepth!==null && /^C\d+(?:[.\w-]*)?(?:\s|[：:])/i.test(title)) {
     const children: RootContent[]=[node];
     while(i+1<tree.children.length) {
      const next=tree.children[i+1];
      if(next.type==="heading" && next.depth<=node.depth)break;
      children.push(next);i++;
     }
     result.push({type:"blockquote",children,data:{hName:"section"}} as RootContent);
     continue;
    }
   }
   result.push(node);
  }
  tree.children=result;
 };
}
