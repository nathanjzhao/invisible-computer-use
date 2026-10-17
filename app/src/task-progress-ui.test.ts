// @vitest-environment jsdom
import {expect,it,vi} from "vitest";
import {renderSavedTasks,type SavedTask} from "./task-progress-ui";
const row:SavedTask={id:"run-test",goal:"Finish <img src=x>",app_name:"Editor",decisions:36,redacted:true,omitted_history:12};
it("loading saved progress requires an explicit click and renders labels as text",()=>{
 const list=document.createElement("div"),load=vi.fn(),discard=vi.fn();renderSavedTasks(list,[row],load,discard,vi.fn());
 expect(load).not.toHaveBeenCalled();expect(list.querySelector("img")).toBeNull();expect(list.textContent).toContain("12 older observations omitted");
 list.querySelector<HTMLButtonElement>("button")!.click();expect(load).toHaveBeenCalledWith(row);expect(discard).not.toHaveBeenCalled();
});
it("keeps failed discards visible and removes confirmed discards",async()=>{
 const list=document.createElement("div"),error=vi.fn(),discard=vi.fn().mockRejectedValueOnce(new Error("task active")).mockResolvedValue(undefined);
 renderSavedTasks(list,[row],vi.fn(),discard,error);const button=list.querySelectorAll<HTMLButtonElement>("button")[1];
 button.click();await new Promise(r=>setTimeout(r,0));expect(error).toHaveBeenCalledWith("Error: task active");expect(list.children).toHaveLength(1);expect(button.disabled).toBe(false);
 button.click();await new Promise(r=>setTimeout(r,0));expect(list.textContent).toBe("No saved task progress.");
});
