import {beforeEach,expect,it} from "vitest";
import {useProjects} from "./projects";
beforeEach(()=>useProjects.setState({projects:[{id:"p",name:"project",path:"/repo",expanded:true,trusted:true,sessions:[]}],activeProjectId:"p",activeSessionId:null}));
it("attaches an existing runtime identity only as UI metadata and deduplicates it",()=>{
 useProjects.getState().attachRuntimeSession("p","cli-run","Existing requirement");useProjects.getState().attachRuntimeSession("p","cli-run","Same run");
 expect(useProjects.getState().projects[0].sessions).toEqual([{id:"cli-run",title:"Existing requirement",state:"draft",updatedAt:expect.any(Number)}]);expect(useProjects.getState().activeSessionId).toBe("cli-run");
});
