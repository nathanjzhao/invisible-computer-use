// @vitest-environment jsdom
import {beforeEach, expect, it} from "vitest";
import {readFileSync} from "node:fs";
import {memorySettingsForm} from "./memory-settings-form";
import type {MemorySettings} from "./memory-ui";

const original: MemorySettings = {chatMemories:true,skysight:false,paused:false,
  retentionDays:7,idleMinutes:30,provider:"",extractModel:null,consolidationModel:null};
const input = (id: string) => document.getElementById(id) as HTMLInputElement;
beforeEach(() => {document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,"utf8");});

it("does not undo another instance's memory-off change after status polling during an edit", () => {
  const form=memorySettingsForm(document);
  form.refresh(original,false);
  input("mem-retention").value="20";
  input("mem-retention").focus();
  form.refresh({...original,chatMemories:false},true);
  expect(input("mem-chat").checked).toBe(true);
  expect(input("mem-retention").value).toBe("20");
  expect(form.patch()).toEqual({retentionDays:20});
});

it("sends an intentional toggle after a complete form refresh", () => {
  const form=memorySettingsForm(document);
  form.refresh({...original,chatMemories:false,paused:true,retentionDays:20},false);
  input("mem-chat").checked=true;
  expect(form.patch()).toEqual({chatMemories:true});
  form.refresh({...original,paused:true,retentionDays:20},false);
  expect(form.patch()).toEqual({});
});

it("preserves a draft after validation failure and a peer update", () => {
  const form=memorySettingsForm(document);
  form.refresh(original,false);
  input("mem-retention").value="0";
  expect(typeof form.patch()).toBe("string");
  form.refresh({...original,skysight:true},true);
  input("mem-retention").value="30";
  expect(form.patch()).toEqual({retentionDays:30});
});
