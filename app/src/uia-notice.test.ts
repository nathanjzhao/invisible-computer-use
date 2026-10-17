import { describe, it, expect } from "vitest";
import { uiAccessNotice, permissionNotice } from "./interact";

describe("uiAccessNotice: a per-user Windows install has no UIAccess helper", () => {
  const base = { supported: false, accessibility: true, screen_recording: true };
  it("shows the backend's note and nothing when there is none", () => {
    const note = "Copilot is installed for this user only, so Windows will not start its UIAccess helper from here.";
    expect(uiAccessNotice({ ...base, uia_note: note })).toBe(note);
    expect(uiAccessNotice({ ...base, uia_note: null })).toBeNull();
    expect(uiAccessNotice(base)).toBeNull();
    expect(uiAccessNotice(null)).toBeNull();
    // It is not a macOS grant, so the Grant access notice stays empty.
    expect(permissionNotice({ ...base, uia_note: note })).toBeNull();
  });
});
