import {expect,it} from "vitest";
import {windowLabel} from "./window-label";
const left={pid:11,window_id:7,app_name:"Fixture",title:"Workflow demo",bounds:{x:80,y:100}};
const right={...left,window_id:8,bounds:{x:680,y:100}};
it("distinguishes same-title windows independently of enumeration order",()=>{
 expect(windowLabel(left,[right,left])).toBe("Left window · Workflow demo");
 expect(windowLabel(right,[right,left])).toBe("Right window · Workflow demo");
});
it("does not merge a different process or title into a position claim",()=>{
 expect(windowLabel(left,[left,{...right,pid:12}])).toBe("Workflow demo");
 expect(windowLabel(left,[left,{...right,title:"Other"}])).toBe("Workflow demo");
});
it("uses upper/lower or an ordinal when left/right cannot distinguish them",()=>{
 const below={...right,bounds:{x:80,y:500}};
 expect(windowLabel(left,[below,left])).toBe("Upper window · Workflow demo");
 const same={...right,bounds:left.bounds};
 expect(windowLabel(left,[same,left])).toBe("Window 1 of 2 · Workflow demo");
});
